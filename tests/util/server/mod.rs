// Copyright 2018-2026 the Deno authors. MIT license.

//! Mock JSR, npm and provenance servers for the tests.

use std::convert::Infallible;

use bytes::Bytes;
use futures::FutureExt;
use futures::future::LocalBoxFuture;
use http::HeaderValue;
use http::Response;
use http_body_util::Empty;
use http_body_util::Full;
use http_body_util::combinators::UnsyncBoxBody;

mod hyper_utils;
mod jsr_registry;
mod npm_registry;

use hyper_utils::ServerKind;
use hyper_utils::ServerOptions;
use hyper_utils::run_server;

use crate::consts;

/// Starts the servers on a background thread, once per process. Returns
/// once they are listening.
pub fn start_servers() {
  static STARTED: std::sync::Once = std::sync::Once::new();
  STARTED.call_once(start_servers_inner);
}

fn start_servers_inner() {
  let (ready_tx, ready_rx) = std::sync::mpsc::channel();
  std::thread::spawn(move || {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async move {
      let mut futures: Vec<LocalBoxFuture<'static, ()>> = vec![
        jsr_registry::registry_server(consts::JSR_REGISTRY_SERVER_PORT)
          .boxed_local(),
        jsr_registry::provenance_mock_server(
          consts::PROVENANCE_MOCK_SERVER_PORT,
        )
        .boxed_local(),
      ];
      futures.extend(npm_registry::public_npm_registry(
        consts::PUBLIC_NPM_REGISTRY_PORT,
      ));
      for future in futures {
        tokio::task::spawn_local(future);
      }
      ready_tx.send(()).unwrap();
      std::future::pending::<()>().await;
    });
  });
  ready_rx.recv().unwrap();
  // wait for the listeners to be bound
  for port in [
    consts::JSR_REGISTRY_SERVER_PORT,
    consts::PROVENANCE_MOCK_SERVER_PORT,
    consts::PUBLIC_NPM_REGISTRY_PORT,
  ] {
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
      std::thread::sleep(std::time::Duration::from_millis(10));
    }
  }
}

fn empty_body() -> UnsyncBoxBody<Bytes, Infallible> {
  UnsyncBoxBody::new(Empty::new())
}

fn string_body(str_: &str) -> UnsyncBoxBody<Bytes, Infallible> {
  UnsyncBoxBody::new(Full::new(Bytes::from(str_.to_string())))
}

fn custom_headers(
  p: &str,
  body: Vec<u8>,
) -> Response<UnsyncBoxBody<Bytes, Infallible>> {
  let mut response = Response::new(UnsyncBoxBody::new(
    http_body_util::Full::new(Bytes::from(body)),
  ));

  let content_type = if p.ends_with(".ts") || p.ends_with(".tsx") {
    Some("application/typescript")
  } else if p.ends_with(".js") || p.ends_with(".jsx") {
    Some("application/javascript")
  } else if p.ends_with(".json") {
    Some("application/json")
  } else if p.ends_with(".wasm") {
    Some("application/wasm")
  } else if p.ends_with(".tgz") {
    Some("application/gzip")
  } else {
    None
  };

  if let Some(t) = content_type {
    response
      .headers_mut()
      .insert("Content-Type", HeaderValue::from_str(t).unwrap());
    return response;
  }

  response
}
