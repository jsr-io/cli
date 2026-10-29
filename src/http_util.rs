// Copyright 2018-2026 the Deno authors. MIT license.

//! HTTP client.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error as AnyError;
use bytes::Bytes;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http_body_util::BodyExt;
use parking_lot::Mutex;
use url::Url;

pub type ResBody = http_body_util::Full<Bytes>;

const USER_AGENT: &str = concat!("jsr-cli/", env!("CARGO_PKG_VERSION"));

/// Creates and caches HTTP clients. A separate client is needed for each
/// client certificate configured in an `.npmrc`.
#[derive(Debug, Default)]
pub struct HttpClientProvider {
  default: Mutex<Option<HttpClient>>,
  with_client_cert: Mutex<HashMap<(PathBuf, PathBuf), HttpClient>>,
}

impl HttpClientProvider {
  pub fn get_or_create(&self) -> Result<HttpClient, AnyError> {
    let mut default = self.default.lock();
    if let Some(client) = default.as_ref() {
      return Ok(client.clone());
    }
    let client = HttpClient::new(create_reqwest_client(None)?);
    *default = Some(client.clone());
    Ok(client)
  }

  pub fn get_or_create_with_client_cert(
    &self,
    certfile: &Path,
    keyfile: &Path,
  ) -> Result<HttpClient, AnyError> {
    let key = (certfile.to_path_buf(), keyfile.to_path_buf());
    let mut clients = self.with_client_cert.lock();
    if let Some(client) = clients.get(&key) {
      return Ok(client.clone());
    }
    let mut pem = std::fs::read(certfile).with_context(|| {
      format!("Failed reading client certificate '{}'", certfile.display())
    })?;
    pem.push(b'\n');
    pem.extend(std::fs::read(keyfile).with_context(|| {
      format!("Failed reading client key '{}'", keyfile.display())
    })?);
    let identity = reqwest::Identity::from_pem(&pem)?;
    let client = HttpClient::new(create_reqwest_client(Some(identity))?);
    clients.insert(key, client.clone());
    Ok(client)
  }
}

fn create_reqwest_client(
  identity: Option<reqwest::Identity>,
) -> Result<reqwest::Client, AnyError> {
  let mut builder = reqwest::Client::builder()
    .user_agent(USER_AGENT)
    // redirects are handled manually so that the authorization header can be
    // stripped when redirecting to a different origin
    .redirect(reqwest::redirect::Policy::none())
    .use_rustls_tls();
  if let Some(identity) = identity {
    builder = builder.identity(identity);
  }
  Ok(builder.build()?)
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
  #[error(transparent)]
  Fetch(reqwest::Error),
  #[error(transparent)]
  Http(#[from] http::Error),
  #[error("Too many redirects.")]
  TooManyRedirects,
  #[error(transparent)]
  RedirectHeaderParse(
    #[from] deno_cache_dir::file_fetcher::RedirectHeaderParseError,
  ),
  #[error("Bad response: {status_code}{}", .response_text.as_ref().map(|s| format!("\n\n{}", s)).unwrap_or_default())]
  BadResponse {
    status_code: http::StatusCode,
    response_text: Option<String>,
  },
}

impl DownloadError {
  fn is_retryable(&self) -> bool {
    matches!(self, Self::BadResponse { .. } | Self::Fetch(_))
  }
}

pub enum HttpClientResponse {
  Success { headers: HeaderMap, body: Vec<u8> },
  NotFound,
  NotModified,
}

#[derive(Debug, Clone)]
pub struct HttpClient {
  client: reqwest::Client,
}

impl HttpClient {
  fn new(client: reqwest::Client) -> Self {
    Self { client }
  }

  pub fn get(&self, url: Url) -> Result<RequestBuilder, http::Error> {
    let mut req = http::Request::new(Bytes::new());
    *req.uri_mut() = url.as_str().parse()?;
    Ok(RequestBuilder {
      client: self.client.clone(),
      req,
    })
  }

  pub fn post(
    &self,
    url: Url,
    body: Bytes,
  ) -> Result<RequestBuilder, http::Error> {
    let mut req = http::Request::new(body);
    *req.method_mut() = http::Method::POST;
    *req.uri_mut() = url.as_str().parse()?;
    Ok(RequestBuilder {
      client: self.client.clone(),
      req,
    })
  }

  pub fn post_json<S>(
    &self,
    url: Url,
    ser: &S,
  ) -> Result<RequestBuilder, AnyError>
  where
    S: serde::Serialize,
  {
    let json = serde_json::to_vec(ser)?;
    let builder = self.post(url, json.into())?;
    Ok(builder.header(
      http::header::CONTENT_TYPE,
      "application/json".parse().map_err(http::Error::from)?,
    ))
  }

  /// Sends a GET request without following redirects.
  pub async fn send(
    &self,
    url: &Url,
    headers: HeaderMap,
  ) -> Result<http::Response<ResBody>, reqwest::Error> {
    let mut req = http::Request::new(Bytes::new());
    *req.uri_mut() = url.as_str().parse().unwrap();
    *req.headers_mut() = headers;
    send_request(&self.client, req).await
  }

  /// Downloads the url, following redirects and retrying on failure. The
  /// response body is not decompressed.
  pub async fn download_with_retries_no_decompress(
    &self,
    url: Url,
    headers: &HeaderMap,
  ) -> Result<HttpClientResponse, DownloadError> {
    crate::util::retry::retry(
      || self.download_inner(url.clone(), headers),
      DownloadError::is_retryable,
    )
    .await
  }

  async fn download_inner(
    &self,
    url: Url,
    headers: &HeaderMap,
  ) -> Result<HttpClientResponse, DownloadError> {
    let response = self.get_redirected_response(url, headers).await?;

    if response.status() == 404 {
      return Ok(HttpClientResponse::NotFound);
    } else if response.status() == 304 {
      return Ok(HttpClientResponse::NotModified);
    } else if !response.status().is_success() {
      let status = response.status();
      let maybe_response_text = body_to_string(response.into_body()).await.ok();
      return Err(DownloadError::BadResponse {
        status_code: status,
        response_text: maybe_response_text
          .map(|s| s.trim().to_string())
          .filter(|s| !s.is_empty()),
      });
    }

    let (parts, body) = response.into_parts();
    Ok(HttpClientResponse::Success {
      headers: parts.headers,
      body: body.collect().await.unwrap().to_bytes().to_vec(),
    })
  }

  async fn get_redirected_response(
    &self,
    mut url: Url,
    headers: &HeaderMap,
  ) -> Result<http::Response<ResBody>, DownloadError> {
    let mut response = self
      .send(&url, headers.clone())
      .await
      .map_err(DownloadError::Fetch)?;
    let status = response.status();
    if status.is_redirection() && status != http::StatusCode::NOT_MODIFIED {
      let mut redirect_headers = headers.clone();
      for _ in 0..5 {
        let new_url =
          deno_cache_dir::file_fetcher::resolve_redirect_from_headers(
            &url,
            response.headers(),
          )
          .map_err(|err| *err)?;
        if should_strip_auth_on_redirect(&url, &new_url) {
          redirect_headers.remove(http::header::AUTHORIZATION);
        }
        let new_response = self
          .send(&new_url, redirect_headers.clone())
          .await
          .map_err(DownloadError::Fetch)?;
        if new_response.status().is_redirection() {
          response = new_response;
          url = new_url;
        } else {
          return Ok(new_response);
        }
      }
      Err(DownloadError::TooManyRedirects)
    } else {
      Ok(response)
    }
  }
}

fn should_strip_auth_on_redirect(original: &Url, new_url: &Url) -> bool {
  new_url.host() != original.host()
    || new_url.port() != original.port()
    || (original.scheme() == "https" && new_url.scheme() == "http")
}

async fn send_request(
  client: &reqwest::Client,
  req: http::Request<Bytes>,
) -> Result<http::Response<ResBody>, reqwest::Error> {
  let req = reqwest::Request::try_from(req.map(reqwest::Body::from))?;
  let response = client.execute(req).await?;
  let mut builder = http::Response::builder()
    .status(response.status())
    .version(response.version());
  if let Some(headers) = builder.headers_mut() {
    *headers = response.headers().clone();
  }
  let body = response.bytes().await?;
  Ok(builder.body(ResBody::new(body)).unwrap())
}

pub async fn body_to_string<B>(body: B) -> Result<String, AnyError>
where
  B: http_body::Body,
  AnyError: From<B::Error>,
{
  let bytes = body.collect().await?.to_bytes();
  let s = std::str::from_utf8(&bytes)?;
  Ok(s.into())
}

pub async fn body_to_json<B, D>(body: B) -> Result<D, AnyError>
where
  B: http_body::Body,
  AnyError: From<B::Error>,
  D: serde::de::DeserializeOwned,
{
  let bytes = body.collect().await?.to_bytes();
  let val = serde_json::from_slice(&bytes)?;
  Ok(val)
}

pub struct RequestBuilder {
  client: reqwest::Client,
  req: http::Request<Bytes>,
}

impl RequestBuilder {
  pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
    self.req.headers_mut().append(name, value);
    self
  }

  pub async fn send(self) -> Result<http::Response<ResBody>, AnyError> {
    Ok(send_request(&self.client, self.req).await?)
  }
}
