// Copyright 2018-2026 the Deno authors. MIT license.

use std::sync::Arc;
use std::time::Duration;

use deno_cache_dir::file_fetcher::SendError;
use deno_cache_dir::file_fetcher::SendResponse;
use deno_terminal::colors;
use http::HeaderMap;
use http_body_util::BodyExt;
use url::Url;

use crate::http_util::HttpClientProvider;

#[derive(Debug, Clone)]
pub struct HttpClientAdapter {
  pub http_client_provider: Arc<HttpClientProvider>,
}

// `deno_cache_dir` declares this trait with `async_trait`, so implementing
// it needs the macro too.
#[async_trait::async_trait(?Send)]
impl deno_cache_dir::file_fetcher::HttpClient for HttpClientAdapter {
  async fn send_no_follow(
    &self,
    url: &Url,
    headers: HeaderMap,
  ) -> Result<SendResponse, SendError> {
    async fn handle_request_or_server_error(
      retried: &mut bool,
      specifier: &Url,
      err_str: String,
    ) -> Result<(), ()> {
      // Retry once, and bail otherwise.
      if !*retried {
        *retried = true;
        log::debug!("Import '{}' failed: {}. Retrying...", specifier, err_str);
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(())
      } else {
        Err(())
      }
    }

    log::info!("{} {}", colors::green("Download"), url);

    let client = self
      .http_client_provider
      .get_or_create()
      .map_err(|err| SendError::Failed(err.into()))?;
    let mut retried = false; // retry intermittent failures
    loop {
      let response = match client.send(url, headers.clone()).await {
        Ok(response) => response,
        Err(err) => {
          if err.is_connect() {
            handle_request_or_server_error(&mut retried, url, err.to_string())
              .await
              .map_err(|()| SendError::Failed(err.into()))?;
            continue;
          } else {
            return Err(SendError::Failed(err.into()));
          }
        }
      };
      let status = response.status();
      if status == http::StatusCode::NOT_MODIFIED {
        return Ok(SendResponse::NotModified);
      }

      if let Some(warning) = response.headers().get("X-Deno-Warning") {
        log::warn!(
          "{} {}",
          colors::yellow("Warning"),
          warning.to_str().unwrap()
        );
      }

      if status.is_redirection() {
        return Ok(SendResponse::Redirect(response.into_parts().0.headers));
      }

      if status.is_server_error() {
        handle_request_or_server_error(&mut retried, url, status.to_string())
          .await
          .map_err(|()| SendError::StatusCode(status))?;
      } else if status.is_client_error() {
        let err = if status == http::StatusCode::NOT_FOUND {
          SendError::NotFound
        } else {
          SendError::StatusCode(status)
        };
        return Err(err);
      } else {
        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes().to_vec();
        return Ok(SendResponse::Success(parts.headers, body));
      }
    }
  }
}
