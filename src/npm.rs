// Copyright 2018-2026 the Deno authors. MIT license.

//! npm registry access, based on `cli/npm.rs` in the Deno repository.

use std::sync::Arc;

use deno_error::JsErrorBox;
use deno_npm_cache::NpmCacheHttpClientBytesResponse;
use deno_npm_cache::NpmCacheHttpClientResponse;
use deno_npm_cache::NpmPackumentFormat;
use deno_npmrc::RegistryConfig;
use deno_terminal::colors;
use url::Url;

use crate::Sys;
use crate::http_util::HttpClientProvider;
use crate::http_util::HttpClientResponse;

pub type NpmInstallerFactory = deno_npm_installer::NpmInstallerFactory<
  NpmCacheHttpClient,
  deno_npm_installer::LogReporter,
  Sys,
>;

#[derive(Debug)]
pub struct NpmCacheHttpClient {
  http_client_provider: Arc<HttpClientProvider>,
  packument_format: NpmPackumentFormat,
}

impl NpmCacheHttpClient {
  pub fn new(
    http_client_provider: Arc<HttpClientProvider>,
    packument_format: NpmPackumentFormat,
  ) -> Self {
    Self {
      http_client_provider,
      packument_format,
    }
  }

  fn get_or_create_http_client(
    &self,
    maybe_registry_config: Option<&RegistryConfig>,
  ) -> Result<crate::http_util::HttpClient, anyhow::Error> {
    if let Some(config) = maybe_registry_config
      && let (Some(certfile), Some(keyfile)) =
        (&config.certfile, &config.keyfile)
    {
      return self.http_client_provider.get_or_create_with_client_cert(
        std::path::Path::new(certfile),
        std::path::Path::new(keyfile),
      );
    }
    self.http_client_provider.get_or_create()
  }
}

// `deno_npm_cache` declares this trait with `async_trait`, so implementing
// it needs the macro too.
#[async_trait::async_trait(?Send)]
impl deno_npm_cache::NpmCacheHttpClient for NpmCacheHttpClient {
  async fn download_with_retries_on_any_tokio_runtime(
    &self,
    url: Url,
    maybe_auth: Option<String>,
    maybe_etag: Option<String>,
    maybe_registry_config: Option<&RegistryConfig>,
  ) -> Result<NpmCacheHttpClientResponse, deno_npm_cache::DownloadError> {
    log::info!("{} {}", colors::green("Download"), url);
    let client = self
      .get_or_create_http_client(maybe_registry_config)
      .map_err(|err| deno_npm_cache::DownloadError {
        status_code: None,
        error: JsErrorBox::generic(format!("{err:#}")),
      })?;
    let mut headers = http::HeaderMap::new();
    if let Some(auth) = maybe_auth {
      headers.append(
        http::header::AUTHORIZATION,
        http::header::HeaderValue::try_from(auth).unwrap(),
      );
    }
    if let Some(etag) = maybe_etag {
      headers.append(
        http::header::IF_NONE_MATCH,
        http::header::HeaderValue::try_from(etag).unwrap(),
      );
    }
    if self.packument_format == NpmPackumentFormat::Abbreviated {
      // Request the abbreviated install manifest when possible. Uses content
      // negotiation with quality factors for registry compatibility (some
      // registries don't support the abbreviated format and need the JSON
      // fallback).
      headers.insert(
        http::header::ACCEPT,
        http::header::HeaderValue::from_static(
          "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        ),
      );
    }
    headers.insert(
      http::header::ACCEPT_ENCODING,
      http::header::HeaderValue::from_static("gzip"),
    );
    let response = client
      .download_with_retries_no_decompress(url, &headers)
      .await
      .map_err(|err| {
        let status_code = match &err {
          crate::http_util::DownloadError::BadResponse {
            status_code, ..
          } => Some(status_code.as_u16()),
          _ => None,
        };
        deno_npm_cache::DownloadError {
          status_code,
          error: JsErrorBox::generic(err.to_string()),
        }
      })?;
    match response {
      HttpClientResponse::Success { headers, body } => {
        // decompress gzip on a blocking thread to keep the event loop free
        let body = if headers
          .get(http::header::CONTENT_ENCODING)
          .and_then(|v| v.to_str().ok())
          .is_some_and(|v| v == "gzip")
        {
          tokio::task::spawn_blocking(move || decompress_gzip(body))
            .await
            .map_err(|e| deno_npm_cache::DownloadError {
              status_code: None,
              error: JsErrorBox::generic(e.to_string()),
            })?
            .map_err(|e| deno_npm_cache::DownloadError {
              status_code: None,
              error: e,
            })?
        } else {
          body
        };
        Ok(NpmCacheHttpClientResponse::Bytes(
          NpmCacheHttpClientBytesResponse {
            etag: headers
              .get(http::header::ETAG)
              .and_then(|e| e.to_str().map(|t| t.to_string()).ok()),
            bytes: body,
          },
        ))
      }
      HttpClientResponse::NotFound => Ok(NpmCacheHttpClientResponse::NotFound),
      HttpClientResponse::NotModified => {
        Ok(NpmCacheHttpClientResponse::NotModified)
      }
    }
  }
}

fn decompress_gzip(compressed: Vec<u8>) -> Result<Vec<u8>, JsErrorBox> {
  use std::io::Read;

  use flate2::read::GzDecoder;
  let mut decoder = GzDecoder::new(compressed.as_slice());
  let mut decompressed = Vec::new();
  decoder.read_to_end(&mut decompressed).map_err(|e| {
    JsErrorBox::generic(format!("gzip decompression failed: {e}"))
  })?;
  Ok(decompressed)
}
