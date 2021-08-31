use std::borrow::Cow;
use std::time::SystemTime;

use crate::{error::*, types::*};

use chrono::{DateTime, Duration, Utc};
use reqwest::{header, IntoUrl, Method, RequestBuilder, Response};
use serde::Serialize;
use snafu::ResultExt;

pub struct ClientOptions {
	pub private_key: String,
	pub installation_login: String,
	pub base_url: String,
	pub base_html_url: String,
}
pub struct Client {
	private_key: Vec<u8>,
	installation_login: String,
	base_url: String,
	base_html_url: String,
}

macro_rules! impl_methods_with_body {
	($($method:ident : $method_response_fn:ident),*) => {
		$(
			pub async fn $method<'b, I, B, T>(&self, url: I, body: &B) -> Result<T>
			where
				I: Into<Cow<'b, str>> + Clone,
				B: Serialize + Clone,
				T: serde::de::DeserializeOwned,
			{
				self.$method_response_fn(url, body)
					.await?
					.json::<T>()
					.await
					.context(HttpSnafu)
			}

			pub async fn $method_response_fn<'b, I, B>(
				&self,
				url: I,
				body: &B,
			) -> Result<Response>
			where
				I: Into<Cow<'b, str>> + Clone,
				B: Serialize + Clone,
			{
				// retry up to N times if request times out
				let mut retries = 0;
				'retry: loop {
					let res = self.execute(
						self.client
						.$method(&*url.clone().into())
						.json(&body.clone()),
					)
					.await;
					// retry if timeout
					if let Err(Error::Http { source: e, .. }) = res.as_ref() {
						if e.is_timeout() && retries < 5 {
							log::debug!("Request timed out; retrying");
							retries += 1;
							continue 'retry;
						}
					}
					return res;
				}
			}

		)*
	}
}

async fn handle_response(response: Response) -> Result<Response> {
	log::debug!("response: {:?}", &response);

	let status = response.status();
	if status.is_success() {
		Ok(response)
	} else {
		let text = response.text().await.context(Http)?;

		// Try to decode the response error as JSON otherwise store
		// it as plain text in a JSON object.
		let body = if let Ok(value) = serde_json::from_str(&text).context(Json)
		{
			value
		} else {
			serde_json::json!({ "error_message": text })
		};

		Response { status, body }.fail()
	}
}

impl Client {
	pub fn new(options: ClientOptions) -> Self {
		let ClientOptions {
			private_key,
			installation_login,
			base_url,
			base_html_url,
		} = options;
		Self {
			private_key: private_key.into(),
			installation_login,
			base_url,
			base_html_url,
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

	pub async fn auth_key(&self) -> Result<String> {
		lazy_static::lazy_static! {
			static ref TOKEN_CACHE: parking_lot::Mutex<Option<(DateTime<Utc>, String)>> = {
				parking_lot::Mutex::new(None)
			};
		}

		let token = {
			TOKEN_CACHE
				.lock()
				.as_ref()
				// Ensure token is not expired if set.
				.filter(|(time, _)| time > &Utc::now())
				.map(|(_, token)| token.clone())
		};

		if let Some(token) = token {
			return Ok(token);
		}

		let installations: Vec<github::Installation> = self
			.jwt_get(&format!("{}/app/installations", self.base_url))
			.await?;

		let installation = installations
			.iter()
			.find(|inst| inst.account.login == self.installation_login)
			.context(MessageSnafu {
				msg: format!(
					"Did not find installation login for {}",
					self.installation_login
				),
			})?;

		let install_token: InstallationToken = self
			.jwt_post(
				&format!(
					"{}/app/installations/{}/access_tokens",
					self.base_url, installation.id
				),
				&serde_json::json!({}),
			)
			.await?;

		let default_exp = Utc::now() + Duration::minutes(40);
		let expiry = install_token
			.expires_at
			.map_or(default_exp, |t| t.parse().unwrap_or(default_exp));
		let token = install_token.token;

		{
			*TOKEN_CACHE.lock() = Some((expiry.clone(), token.clone()))
		};

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
			.timeout(std::time::Duration::from_secs(10))
			.build()
			.context(ReqwestSnafu)?;

		log::debug!("request: {:?}", &request);
		handle_response(self.client.execute(request).await.context(Http)?).await
	}

	fn create_jwt(&self) -> Result<String> {
		log::debug!("create_jwt");
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
		.context(JwtSnafu)
	}

	async fn jwt_execute(&self, builder: RequestBuilder) -> Result<Response> {
		log::debug!("jwt_execute");
		let response = builder
			.bearer_auth(&self.create_jwt()?)
			.header(
				header::ACCEPT,
				"application/vnd.github.machine-man-preview+json",
			)
			.header(header::USER_AGENT, "parity-processbot/0.0.1")
			.timeout(std::time::Duration::from_secs(10))
			.send()
			.await
			.context(Http)?;

		handle_response(response).await
	}

	pub async fn jwt_get<T>(&self, url: impl IntoUrl) -> Result<T>
	where
		T: serde::de::DeserializeOwned,
	{
		log::debug!("jwt_get");
		self.jwt_execute(self.client.get(url))
			.await?
			.json::<T>()
			.await
			.context(ReqwestSnafu)
	}

	pub async fn jwt_post<T>(
		&self,
		url: impl IntoUrl,
		body: &impl serde::Serialize,
	) -> Result<T>
	where
		T: serde::de::DeserializeOwned,
	{
		log::debug!("jwt_post");
		self.jwt_execute(self.client.post(url).json(body))
			.await?
			.json::<T>()
			.await
			.context(ReqwestSnafu)
	}

	pub async fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>> + Clone,
		T: serde::de::DeserializeOwned + core::fmt::Debug,
	{
		self.get_response(url, serde_json::json!({}))
			.await?
			.json::<T>()
			.await
			.context(ReqwestSnafu)
	}

	pub async fn get_status<'b, I>(&self, url: I) -> Result<u16>
	where
		I: Into<Cow<'b, str>> + Clone,
	{
		let res = self
			.get_response(url, serde_json::json!({}))
			.await?
			.status()
			.as_u16();
		Ok(res)
	}

	// Originally adapted from:
	// https://github.com/XAMPPRocky/gh-auditor/blob/ca67641c0a29d64fc5c6b4244b45ae601604f3c1/src/lib.rs#L232-L267
	/// Gets a all entries across all pages from a resource in GitHub.
	pub async fn get_all<'b, I, T>(&self, url: I) -> Result<Vec<T>>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned + core::fmt::Debug,
	{
		log::debug!("get_all");
		let mut entities = Vec::new();
		let mut next = Some(url.into());

		while let Some(url) = next {
			log::debug!("getting next");
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

			let mut body = response.json::<Vec<T>>().await.context(Http)?;
			entities.append(&mut body);
		}

		Ok(entities)
	}
}
