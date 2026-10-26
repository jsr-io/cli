// Copyright 2018-2026 the Deno authors. MIT license.

use std::convert::Infallible;
use std::future::Future;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV6;
use std::path::PathBuf;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use bytes::Bytes;
use futures::FutureExt;
use futures::future::LocalBoxFuture;
use http::HeaderMap;
use http::HeaderValue;
use http_body_util::combinators::UnsyncBoxBody;
use hyper::Request;
use hyper::Response;
use hyper::StatusCode;
use hyper::body::Incoming;
use serde_json::json;
use sha2::Digest;

use super::ServerKind;
use super::ServerOptions;
use super::custom_headers;
use super::empty_body;
use super::hyper_utils::HandlerOutput;
use super::run_server;
use super::string_body;
use crate::npm;

pub fn public_npm_registry(port: u16) -> Vec<LocalBoxFuture<'static, ()>> {
  run_npm_server(port, "npm registry server error", {
    move |req| async move {
      handle_req_for_registry(req, &npm::PUBLIC_TEST_NPM_REGISTRY).await
    }
  })
}

fn run_npm_server<F, S>(
  port: u16,
  error_msg: &'static str,
  handler: F,
) -> Vec<LocalBoxFuture<'static, ()>>
where
  F: Fn(Request<hyper::body::Incoming>) -> S + Copy + 'static,
  S: Future<Output = HandlerOutput> + 'static,
{
  let npm_registry_addr = SocketAddr::from(([127, 0, 0, 1], port));
  let ipv6_loopback = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
  let npm_registry_ipv6_addr =
    SocketAddr::V6(SocketAddrV6::new(ipv6_loopback, port, 0, 0));
  vec![
    run_npm_server_for_addr(npm_registry_addr, error_msg, handler)
      .boxed_local(),
    // necessary because the npm binary will sometimes resolve localhost to ::1
    run_npm_server_for_addr(npm_registry_ipv6_addr, error_msg, handler)
      .boxed_local(),
  ]
}

async fn run_npm_server_for_addr<F, S>(
  addr: SocketAddr,
  error_msg: &'static str,
  handler: F,
) where
  F: Fn(Request<hyper::body::Incoming>) -> S + Copy + 'static,
  S: Future<Output = HandlerOutput> + 'static,
{
  run_server(
    ServerOptions {
      addr,
      kind: ServerKind::Auto,
      error_msg,
    },
    handler,
  )
  .await
}

async fn handle_req_for_registry(
  req: Request<Incoming>,
  test_npm_registry: &npm::TestNpmRegistry,
) -> Result<Response<UnsyncBoxBody<Bytes, Infallible>>, anyhow::Error> {
  let root_dir = test_npm_registry.root_dir();

  // serve the registry package files
  let uri_path = req.uri().path();

  // Package requests may arrive through a registry url configured with a
  // sub-path (e.g. `http://localhost:4260/sub/path/`). Strip the known test
  // sub-path prefix so the same package files are served. This lets tests
  // exercise sub-path registry cache layouts (the on-disk cache ends up under
  // `<host>/sub/path/<pkg>` rather than `<host>/<pkg>`).
  let uri_path = uri_path.strip_prefix("/sub/path").unwrap_or(uri_path);

  let mut file_path = root_dir.to_path_buf();
  file_path.push(uri_path[1..].replace("%2f", "/").replace("%2F", "/"));

  // serve if the filepath exists
  if let Ok(file) = tokio::fs::read(&file_path).await {
    let file_resp = custom_headers(uri_path, file);
    return Ok(file_resp);
  }

  // otherwise try to serve from the registry
  if let Some(resp) = try_serve_npm_registry(
    uri_path,
    file_path.clone(),
    req.headers(),
    test_npm_registry,
  )
  .await
  {
    return resp;
  }

  Response::builder()
    .status(StatusCode::NOT_FOUND)
    .body(empty_body())
    .map_err(|e| e.into())
}

/// Returns true if the Accept header indicates the client wants the
/// abbreviated install manifest (`application/vnd.npm.install-v1+json`).
fn wants_abbreviated(headers: &HeaderMap<HeaderValue>) -> bool {
  headers
    .get(http::header::ACCEPT)
    .and_then(|v| v.to_str().ok())
    .map(|v| v.contains("application/vnd.npm.install-v1+json"))
    .unwrap_or(false)
}

/// Converts a full packument JSON into the abbreviated install manifest format.
/// Strips `time` and `scripts`, replaces scripts with `hasInstallScript`.
fn to_abbreviated_packument(full: &[u8]) -> Vec<u8> {
  let mut packument: serde_json::Value = serde_json::from_slice(full).unwrap();
  let obj = packument.as_object_mut().unwrap();

  // Remove `time` (not present in abbreviated format)
  obj.remove("time");

  // Transform each version entry
  if let Some(versions) =
    obj.get_mut("versions").and_then(|v| v.as_object_mut())
  {
    for version_info in versions.values_mut() {
      let vi = version_info.as_object_mut().unwrap();

      // Check for install lifecycle scripts before removing
      let has_install_script = vi
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|scripts| {
          scripts.contains_key("preinstall")
            || scripts.contains_key("install")
            || scripts.contains_key("postinstall")
        })
        .unwrap_or(false);

      // Remove `scripts` (not present in abbreviated format)
      vi.remove("scripts");

      // Add `hasInstallScript` if applicable
      if has_install_script {
        vi.insert("hasInstallScript".to_string(), json!(true));
      }
    }
  }

  serde_json::to_vec(&packument).unwrap()
}

fn handle_custom_npm_registry_path(
  scope_name: &str,
  path: &str,
  headers: &HeaderMap<HeaderValue>,
  test_npm_registry: &npm::TestNpmRegistry,
) -> Result<Option<Response<UnsyncBoxBody<Bytes, Infallible>>>, anyhow::Error> {
  let mut parts = path
    .split('/')
    .filter(|p| !p.is_empty())
    .collect::<Vec<_>>();
  let remainder = parts.split_off(1);
  let name = parts[0];
  let package_name = format!("{}/{}", scope_name, name);

  if remainder.len() == 1 {
    if let Some(file_bytes) = test_npm_registry
      .tarball_bytes(&package_name, remainder[0].trim_end_matches(".tgz"))?
    {
      let file_resp = custom_headers("file.tgz", file_bytes);
      return Ok(Some(file_resp));
    }
  } else if remainder.is_empty()
    && let Some(registry_file) =
      test_npm_registry.registry_file(&package_name)?
  {
    let registry_file = if wants_abbreviated(headers) {
      to_abbreviated_packument(&registry_file)
    } else {
      registry_file
    };

    let actual_etag = format!(
      "\"{}\"",
      BASE64_STANDARD.encode(sha2::Sha256::digest(&registry_file))
    );
    if headers.get("If-None-Match").and_then(|v| v.to_str().ok())
      == Some(actual_etag.as_str())
    {
      let mut response = Response::new(UnsyncBoxBody::new(
        http_body_util::Full::new(Bytes::from(vec![])),
      ));
      *response.status_mut() = StatusCode::NOT_MODIFIED;
      return Ok(Some(response));
    }

    let mut file_resp = custom_headers("registry.json", registry_file);
    file_resp.headers_mut().append(
      http::header::ETAG,
      http::header::HeaderValue::from_str(&actual_etag).unwrap(),
    );

    return Ok(Some(file_resp));
  }

  Ok(None)
}

async fn try_serve_npm_registry(
  uri_path: &str,
  mut testdata_file_path: PathBuf,
  headers: &HeaderMap<HeaderValue>,
  test_npm_registry: &npm::TestNpmRegistry,
) -> Option<Result<Response<UnsyncBoxBody<Bytes, Infallible>>, anyhow::Error>> {
  if let Some((scope_name, package_name_with_path)) = test_npm_registry
    .get_test_scope_and_package_name_with_path_from_uri_path(uri_path)
  {
    // serve all requests to the `DENOTEST_SCOPE_NAME` or `DENOTEST2_SCOPE_NAME`
    // using the file system at that path
    match handle_custom_npm_registry_path(
      scope_name,
      package_name_with_path,
      headers,
      test_npm_registry,
    ) {
      Ok(Some(response)) => return Some(Ok(response)),
      Ok(None) => {} // ignore, not found
      Err(err) => {
        return Some(
          Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(string_body(&format!("{err:#}")))
            .map_err(|e| e.into()),
        );
      }
    }
  } else {
    // otherwise, serve based on registry.json and tgz files
    let is_tarball = uri_path.ends_with(".tgz");
    if !is_tarball {
      testdata_file_path.push("registry.json");
    }
    if let Ok(file) = tokio::fs::read(&testdata_file_path).await {
      let file = if !is_tarball && wants_abbreviated(headers) {
        to_abbreviated_packument(&file)
      } else {
        file
      };
      let file_resp = custom_headers(uri_path, file);
      return Some(Ok(file_resp));
    }
  }

  None
}
