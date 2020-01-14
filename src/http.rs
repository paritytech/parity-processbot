use std::borrow::Cow;

use crate::{error, Result};

use hyperx::header::TypedHeaders;
use serde::Serialize;
use snafu::ResultExt;

pub struct Client {
	client: reqwest::Client,
	auth_key: String,
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
            ) -> Result<reqwest::Response>
            where
                I: Into<Cow<'b, str>>,
                B: Serialize,
            {
                self.request(
                    self.client
                    .$method(&*url.into())
                    .json(body),
                )
                    .await
            }

        )*
    }
}

/// HTTP util methods.
impl Client {
	pub fn new<I: Into<String>>(auth_key: I) -> Self {
		Self {
			client: reqwest::Client::new(),
			auth_key: auth_key.into(),
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
		builder: reqwest::RequestBuilder,
	) -> Result<reqwest::Response> {
		let request = builder
			.bearer_auth(&self.auth_key)
			.header(
				reqwest::header::ACCEPT,
				"application/vnd.github.starfox-preview+json",
			)
			.header(reqwest::header::USER_AGENT, "parity-processbot/0.0.1")
			.build()
			.context(error::Http)?;

		dbg!(&request);

		let response =
			self.client.execute(request).await.context(error::Http)?;
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

	/// Sends a `GET` request to `url`, supplying the relevant headers for
	/// authenication and feature detection.
	async fn get_response<'b, I: Into<Cow<'b, str>>>(
		&self,
		url: I,
	) -> Result<reqwest::Response> {
		self.request(self.client.get(&*url.into())).await
	}

	/// Get a single entry from a resource in GitHub.
	pub async fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		self.get_response(url)
			.await?
			.json::<T>()
			.await
			.context(error::Http)
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
			let response = self.get_response(url).await?;

			next = response
				.headers()
				.decode::<hyperx::header::Link>()
				.ok()
				.and_then(|v| {
					v.values()
						.iter()
						.find(|link| {
							link.rel()
								.map(|rel| {
									rel.contains(
										&hyperx::header::RelationType::Next,
									)
								})
								.unwrap_or(false)
						})
						.map(|l| l.link())
						.map(str::to_owned)
						.map(Cow::Owned)
				});

			let mut body =
				response.json::<Vec<T>>().await.context(error::Http)?;
			entities.append(&mut body);
		}

		Ok(entities)
	}
}
