use crate::webhook::*;
use anyhow::{Context, Result};
use async_std::pin::Pin;
use futures_util::{future::Future, FutureExt};
use futures_util::{
	io::{AsyncRead, AsyncWrite},
	stream::Stream,
};
use hyper::http::StatusCode;
use hyper::{
	service::{make_service_fn, service_fn},
	Body, Request, Response, Server,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;

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

#[derive(Debug)]
pub enum Error {
	/// Hyper internal error.
	Hyper(hyper::Error),
	/// Http request error.
	Http(hyper::http::Error),
	/// i/o error.
	Io(std::io::Error),
	PortInUse(SocketAddr),
}

impl std::fmt::Display for Error {
	fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(fmt, "{:?}", self)
	}
}

impl std::error::Error for Error {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Error::Hyper(error) => Some(error),
			Error::Http(error) => Some(error),
			Error::Io(error) => Some(error),
			Error::PortInUse(_) => None,
		}
	}
}

#[derive(Clone)]
pub struct Executor;

impl<T> hyper::rt::Executor<T> for Executor
where
	T: Future + Send + 'static,
	T::Output: Send + 'static,
{
	fn execute(&self, future: T) {
		async_std::task::spawn(future);
	}
}

/// Initializes the metrics context, and starts an HTTP server
/// to serve metrics.
pub async fn init_server(
	addr: SocketAddr,
	state: Arc<AppState>,
) -> anyhow::Result<()> {
	let listener = async_std::net::TcpListener::bind(&addr)
		.await
		.map_err(|_| Error::PortInUse(addr))?;

	log::info!("Listening on {}", addr);

	let service = make_service_fn(move |_| {
		let state = Arc::clone(&state);
		async move {
			Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
				let state = parking_lot::Mutex::new(Arc::clone(&state));
				webhook(req, state)
			}))
		}
	});

	let server = Server::builder(Incoming(listener.incoming()))
//		.executor(Executor)
		.serve(service)
		.boxed();

	let result = server.await.context(format!(""));

	result
}
