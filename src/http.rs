use std::borrow::Cow;
use std::time::SystemTime;

use crate::{error, github, Result};

use chrono::{DateTime, Duration, Utc};
use hyperx::header::TypedHeaders;
use reqwest::{header, IntoUrl, Method, RequestBuilder, Response};
use serde::Serialize;
use snafu::{OptionExt, ResultExt};

#[derive(Default)]
pub struct Client {
	pub client: reqwest::Client,
	private_key: Vec<u8>,
	installation_login: String,
}

macro_rules! impl_methods_with_body {
    ($($method:ident : $method_response_fn:ident),*) => {
        $(
            pub async fn $method<'b, I, B, T>(&self, url: I, body: &B) -> Result<T>
            where
                I: Into<Cow<'b, str>>,
                B: Serialize,
                T: serde::de::DeserializeOwned,
            {
                self.$method_response_fn(url, body)
                    .await?
                    .json::<T>()
                    .await
                    .context(error::Http)
            }

            pub async fn $method_response_fn<'b, I, B>(
                &self,
                url: I,
                body: &B,
            ) -> Result<Response>
            where
                I: Into<Cow<'b, str>>,
                B: Serialize,
            {
                self.execute(
                    self.client
                    .$method(&*url.into())
                    .json(body),
                )
                    .await
            }

        )*
    }
}

/// Checks the response's status and maps into an `Err` branch if
/// not successful.
async fn handle_response(response: Response) -> Result<Response> {
	let status = response.status();

	if status.is_success() {
		Ok(response)
	} else {
		let text = response.text().await.context(error::Http)?;

		// Try to decode the response error as JSON otherwise store
		// it as plain text in a JSON object.
		let body = if let Ok(value) =
			serde_json::from_str(&text).context(error::Json)
		{
			value
		} else {
			serde_json::json!({ "error_message": text })
		};

		error::Response { status, body }.fail()
	}
}

/// HTTP util methods.
impl Client {
	pub fn new(private_key: Vec<u8>, installation_login: String) -> Self {
		Self {
			private_key: private_key.into(),
			installation_login,
			..Self::default()
		}
	}

	impl_methods_with_body! {
		post: post_response,
		put: put_response,
		patch: patch_response,
		delete: delete_response
	}

	pub async fn request(
		&self,
		method: Method,
		url: impl IntoUrl,
	) -> RequestBuilder {
		self.client.request(method, url)
	}

	async fn auth_key(&self) -> Result<String> {
		lazy_static::lazy_static! {
			static ref TOKEN_CACHE: std::sync::RwLock<Option<(DateTime<Utc>, String)>> = {
				std::sync::RwLock::new(None)
			};
		}

		let token = TOKEN_CACHE
			.read()
			.as_ref()
			.unwrap()
			.as_ref()
			// Ensure token is not expired if set.
			.filter(|(time, _)| time > &Utc::now())
			.map(|(_, token)| token.clone());

		if let Some(token) = token {
			return Ok(token);
		}

		let installations: Vec<github::Installation> = self
			.jwt_get(&format!(
				"{}/app/installations",
				crate::github_bot::GithubBot::BASE_URL
			))
			.await?;

		let installation = installations
			.iter()
			.find(|inst| inst.account.login == self.installation_login)
			.context(error::MissingData)?;

		let install_token: github::InstallationToken = self
			.jwt_post(
				&format!(
					"{}/app/installations/{}/access_tokens",
					crate::github_bot::GithubBot::BASE_URL,
					installation.id
				),
				&serde_json::json!({}),
			)
			.await?;

		let mut write = TOKEN_CACHE.write().unwrap();

		let token = install_token.token.clone();
		let default_exp = Utc::now() + Duration::minutes(40);
		*write = Some((
			install_token
				.expires_at
				.map_or(default_exp, |t| t.parse().unwrap_or(default_exp)),
			install_token.token,
		));

		Ok(token)
	}

	async fn execute(&self, builder: RequestBuilder) -> Result<Response> {
		let request = builder
			.bearer_auth(&self.auth_key().await?)
			.header(
				header::ACCEPT,
				"application/vnd.github.starfox-preview+json",
			)
			.header(
				header::ACCEPT,
				"application/vnd.github.inertia-preview+json",
			)
			.header(
				header::ACCEPT,
				"application/vnd.github.antiope-preview+json",
			)
			.header(
				header::ACCEPT,
				"application/vnd.github.machine-man-preview+json",
			)
			.header(header::USER_AGENT, "parity-processbot/0.0.1")
			.timeout(std::time::Duration::from_secs(5))
			.build()
			.context(error::Http)?;

		log::debug!("{:?}", &request);

		handle_response(
			self.client.execute(request).await.context(error::Http)?,
		)
		.await
	}

	fn create_jwt(&self) -> Result<String> {
		const TEN_MINS_IN_SECONDS: u64 = 10 * 60;
		lazy_static::lazy_static! {
			static ref APP_ID: u64 = dotenv::var("GITHUB_APP_ID").unwrap().parse::<u64>().unwrap();
		}
		let iat = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let body = serde_json::json!({
			"iat": iat,
			"exp": iat + TEN_MINS_IN_SECONDS,
			"iss": *APP_ID,
		});

		jsonwebtoken::encode(
			&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
			&body,
			&jsonwebtoken::EncodingKey::from_rsa_pem(&self.private_key)
				.expect("private key should be RSA pem"),
		)
		.context(error::Jwt)
	}

	async fn jwt_execute(&self, builder: RequestBuilder) -> Result<Response> {
		let response = builder
			.bearer_auth(&self.create_jwt()?)
			.header(
				header::ACCEPT,
				"application/vnd.github.machine-man-preview+json",
			)
			.header(header::USER_AGENT, "parity-processbot/0.0.1")
			.send()
			.await
			.context(error::Http)?;

		handle_response(response).await
	}

	/// Get a single entry from a resource in GitHub using
	/// JWT authenication.
	pub async fn jwt_get<T>(&self, url: impl IntoUrl) -> Result<T>
	where
		T: serde::de::DeserializeOwned,
	{
		self.jwt_execute(self.client.get(url))
			.await?
			.json::<T>()
			.await
			.context(error::Http)
	}

	/// Posts `body` GitHub to `url` using JWT authenication.
	pub async fn jwt_post<T>(
		&self,
		url: impl IntoUrl,
		body: &impl serde::Serialize,
	) -> Result<T>
	where
		T: serde::de::DeserializeOwned,
	{
		self.jwt_execute(self.client.post(url).json(body))
			.await?
			.json::<T>()
			.await
			.context(error::Http)
	}

	/// Get a single entry from a resource in GitHub.
	pub async fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>> + Clone,
		T: serde::de::DeserializeOwned,
	{
		self.get_response(url, serde_json::json!({}))
			.await?
			.json::<T>()
			.await
			.context(error::Http)
	}

	/// Get a single entry from a resource in GitHub. TODO fix
	pub async fn get_with_params<'b, I, T, P>(
		&self,
		url: I,
		params: P,
	) -> Result<T>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
		P: Serialize,
	{
		self.get_response(url, params)
			.await?
			.json::<T>()
			.await
			.context(error::Http)
	}

	//	/// Sends a `GET` request to `url`, supplying the relevant headers for
	//	/// authenication and feature detection.
	pub async fn get_response<'b, I: Into<Cow<'b, str>>, P: Serialize>(
		&self,
		url: I,
		params: P,
	) -> Result<Response> {
		self.execute(self.client.get(&*url.into()).json(&params))
			.await
	}

	// Originally adapted from:
	// https://github.com/XAMPPRocky/gh-auditor/blob/ca67641c0a29d64fc5c6b4244b45ae601604f3c1/src/lib.rs#L232-L267
	/// Gets a all entries across all pages from a resource in GitHub.
	pub async fn get_all<'b, I, T>(&self, url: I) -> Result<Vec<T>>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		let mut entities = Vec::new();
		let mut next = Some(url.into());

		while let Some(url) = next {
			let response =
				self.get_response(url, serde_json::json!({})).await?;

			next = response
				.headers()
				.decode::<hyperx::header::Link>()
				.ok()
				.iter()
				.flat_map(|v| v.values())
				.find(|link| {
					link.rel().map_or(false, |rel| {
						rel.contains(&hyperx::header::RelationType::Next)
					})
				})
				.map(|l| l.link())
				.map(str::to_owned)
				.map(Cow::Owned);

			let mut body =
				response.json::<Vec<T>>().await.context(error::Http)?;
			entities.append(&mut body);
		}

		Ok(entities)
	}
}
