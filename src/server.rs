use crate::{error::*, webhook, AppState};
use async_std::pin::Pin;
use futures_util::{
	io::{self, AsyncRead, AsyncWrite},
	stream::Stream,
};
use hyper::{
	service::{make_service_fn, service_fn},
	Body, Request, Response, Server,
};
use reqwest::Response;
use snafu::ResultExt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use tokio::sync::Mutex;

pub struct Incoming<'a>(pub async_std::net::Incoming<'a>);

impl hyper::server::accept::Accept for Incoming<'_> {
	type Conn = TcpStream;
	type Error = async_std::io::Error;

	fn poll_accept(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context,
	) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
		Pin::new(&mut Pin::into_inner(self).0)
			.poll_next(cx)
			.map(|opt| opt.map(|res| res.map(TcpStream)))
	}
}

pub struct TcpStream(pub async_std::net::TcpStream);

impl tokio::io::AsyncRead for TcpStream {
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context,
		buf: &mut [u8],
	) -> Poll<Result<usize, std::io::Error>> {
		Pin::new(&mut Pin::into_inner(self).0).poll_read(cx, buf)
	}
}

impl tokio::io::AsyncWrite for TcpStream {
	fn poll_write(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context,
		buf: &[u8],
	) -> Poll<Result<usize, std::io::Error>> {
		Pin::new(&mut Pin::into_inner(self).0).poll_write(cx, buf)
	}

	fn poll_flush(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context,
	) -> Poll<Result<(), std::io::Error>> {
		Pin::new(&mut Pin::into_inner(self).0).poll_flush(cx)
	}

	fn poll_shutdown(
		self: Pin<&mut Self>,
		cx: &mut std::task::Context,
	) -> Poll<Result<(), std::io::Error>> {
		Pin::new(&mut Pin::into_inner(self).0).poll_close(cx)
	}
}

pub async fn init_server(
	addr: SocketAddr,
	state: Arc<Mutex<AppState>>,
) -> Result<()> {
	let listener = async_std::net::TcpListener::bind(&addr)
		.await
		.context(IoSnafu)?;

	let service = make_service_fn(move |_| {
		let state = Arc::clone(&state);
		async move {
			Ok::<_, hyper::Error>(service_fn(
				async move |req: Request<Body>| {
					let result = match req.uri().path() {
						"/webhook" => {
							match webhook::handle_request(req, state).await {
								Ok(_) => Response::builder()
									.status(StatusCode::OK)
									.body(Body::from(""))
									.context(HttpSnafu),
								Err(e) => Err(e),
							}
						}
						_ => Ok(Response::builder()
							.status(StatusCode::NOT_FOUND)
							.body(Body::from("Not found."))
							.context(HttpSnafu)),
					};

					match result.flatten() {
						Ok(response) => response,
						Err(error) => handle_error(e, state).await,
					};
				},
			));
		}
	});

	log::info!("Listening on {}", addr);

	Server::builder(Incoming(listener.incoming()))
		.http1_half_close(true)
		.serve(service)
		.await
		.map(|_| ())
		.context(HyperSnafu)
}
