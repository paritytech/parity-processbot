use std::{net::SocketAddr, sync::Arc, task::Poll};

use anyhow::{Context, Result};
use async_std::pin::Pin;
use futures_util::{
	io::{AsyncRead, AsyncWrite},
	stream::Stream,
	FutureExt,
};
use hyper::{
	service::{make_service_fn, service_fn},
	Body, Request, Server,
};
use tokio::sync::Mutex;

use crate::{bot::*, core::AppState};

struct Incoming<'a>(pub async_std::net::Incoming<'a>);

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

struct TcpStream(pub async_std::net::TcpStream);

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

pub async fn init(
	addr: SocketAddr,
	state: Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
	let listener = async_std::net::TcpListener::bind(&addr).await.unwrap();

	log::info!("Listening on {}", addr);

	let service = make_service_fn(move |_| {
		let state = Arc::clone(&state);
		async move {
			Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
				let state = Arc::clone(&state);
				handle_http_request_for_bot(req, state)
			}))
		}
	});

	Server::builder(Incoming(listener.incoming()))
		.http1_half_close(true)
		.serve(service)
		.boxed()
		.await
		.context("Server error".to_owned())
}
