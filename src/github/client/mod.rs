use std::{borrow::Cow, time::SystemTime};

use chrono::{DateTime, Duration, Utc};
use reqwest::{header, IntoUrl, RequestBuilder, Response};
use serde::Serialize;
use snafu::ResultExt;

mod commit;
mod file;
mod issue;
mod org;
mod pull_request;

use crate::{
	config::MainConfig,
	error::{self, Error},
	github,
	types::Result,
};

pub struct GithubClient {
	client: reqwest::Client,
	private_key: Vec<u8>,
	installation_login: String,
	github_app_id: usize,
	github_api_url: String,
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
					.context(error::Http)
			}

			async fn $method_response_fn<'b, I, B>(
				&self,
				url: I,
				body: &B,
			) -> Result<Response>
			where
				I: Into<Cow<'b, str>> + Clone,
				B: Serialize + Clone,
			{
				// retry up to 5 times if request times out
				let mut retries = 0;
				'retry: loop {
					let res = self.execute(
						self.client
						.$method(&*url.clone().into())
						.json(&body.clone()),
					)
					.await;
					// retry if timeout
					if let Err(error::Error::Http { source: e, .. }) = res.as_ref() {
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

impl GithubClient {
	pub fn new(config: &MainConfig) -> Self {
		Self {
			private_key: config.private_key.clone(),
			installation_login: config.installation_login.clone(),
			github_app_id: config.github_app_id,
			github_api_url: config.github_api_url.clone(),
			client: reqwest::Client::default(),
		}
	}

	impl_methods_with_body! {
		post: post_response,
		put: put_response,
		patch: patch_response,
		delete: delete_response
	}

	pub async fn auth_token(&self) -> Result<String> {
		log::debug!("auth_token");

		lazy_static::lazy_static! {
			static ref TOKEN_CACHE: parking_lot::Mutex<Option<(DateTime<Utc>, String)>> = {
				parking_lot::Mutex::new(None)
			};
		}

		// Add some padding for avoiding token use just as it's about to expire
		let installation_lease_with_padding =
			Utc::now() + Duration::minutes(10);
		let token = {
			TOKEN_CACHE
				.lock()
				.as_ref()
				// Ensure token is not expired if set.
				.filter(|(time, _)| time > &installation_lease_with_padding)
				.map(|(_, token)| token.clone())
		};

		if let Some(token) = token {
			return Ok(token);
		}

		let installations: Vec<github::GithubInstallation> = self
			.jwt_get(&format!("{}/app/installations", self.github_api_url,))
			.await?;

		let installation = if let Some(installation) = installations
			.iter()
			.find(|inst| inst.account.login == self.installation_login)
		{
			installation
		} else {
			return Err(Error::Message {
				msg: format!(
					"Installation for login {} could not be found",
					self.installation_login
				),
			});
		};

		let install_token: github::GithubInstallationToken = self
			.jwt_post(
				&format!(
					"{}/app/installations/{}/access_tokens",
					self.github_api_url, installation.id
				),
				&serde_json::json!({}),
			)
			.await?;

		let default_exp = Utc::now() + Duration::minutes(40);
		let expiry = install_token
			.expires_at
			.map_or(default_exp, |t| t.parse().unwrap_or(default_exp));
		let token = install_token.token;

		*TOKEN_CACHE.lock() = Some((expiry, token.clone()));

		Ok(token)
	}

	async fn execute(&self, builder: RequestBuilder) -> Result<Response> {
		let request = builder
			.bearer_auth(&self.auth_token().await?)
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
			.context(error::Http)?;

		log::debug!("request: {:?}", &request);
		handle_response(
			self.client.execute(request).await.context(error::Http)?,
		)
		.await
	}

	fn create_jwt(&self) -> Result<String> {
		log::debug!("create_jwt");
		const TEN_MINS_IN_SECONDS: u64 = 10 * 60;
		let iat = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let body = serde_json::json!({
			"iat": iat,
			"exp": iat + TEN_MINS_IN_SECONDS,
			"iss": self.github_app_id,
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
			.context(error::Http)?;

		handle_response(response).await
	}

	async fn jwt_get<T>(&self, url: impl IntoUrl) -> Result<T>
	where
		T: serde::de::DeserializeOwned,
	{
		log::debug!("jwt_get");
		self.jwt_execute(self.client.get(url))
			.await?
			.json::<T>()
			.await
			.context(error::Http)
	}

	async fn jwt_post<T>(
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
			.context(error::Http)
	}

	async fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>> + Clone,
		T: serde::de::DeserializeOwned + core::fmt::Debug,
	{
		
		self
			.get_response(url, serde_json::json!({}))
			.await?
			.json::<T>()
			.await
			.context(error::Http)
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

	async fn get_response<'b, I, P>(
		&self,
		url: I,
		params: P,
	) -> Result<Response>
	where
		I: Into<Cow<'b, str>> + Clone,
		P: Serialize + Clone,
	{
		log::debug!("get_response");
		// retry up to 5 times if request times out
		let mut retries = 0;
		'retry: loop {
			let res = self
				.execute(
					self.client.get(&*url.clone().into()).json(&params.clone()),
				)
				.await;
			// retry if timeout
			if let Err(error::Error::Http { source: e, .. }) = res.as_ref() {
				if e.is_timeout() && retries < 5 {
					log::debug!("Request timed out; retrying");
					retries += 1;
					continue 'retry;
				}
			}
			return res;
		}
	}
}
