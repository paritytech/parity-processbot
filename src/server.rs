use std::{net::SocketAddr, sync::Arc};




use hyper::{
	service::{make_service_fn, service_fn},
	Body, Request, Server,
};
use tokio::sync::Mutex;

use crate::{bot::*, core::AppState};

pub async fn init(
	addr: SocketAddr,
	state: Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
	let service = make_service_fn(move |_| {
		let state = Arc::clone(&state);
		async move {
			Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
				let state = Arc::clone(&state);
				handle_http_request_for_bot(req, state)
			}))
		}
	});

	let server = Server::bind(&addr).http1_half_close(true).serve(service);

	if let Err(e) = server.await {
		eprintln!("server error: {}", e);
	}

	Ok(())
}
