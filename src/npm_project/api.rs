// Copyright the JSR authors. MIT license.

use std::collections::HashMap;

use anyhow::Error as AnyError;
use anyhow::bail;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use super::JsrPackage;
use crate::http_util::HttpClient;
use crate::http_util::body_to_json;

#[derive(Debug, Deserialize)]
pub struct PackageMeta {
  pub latest: Option<String>,
  pub versions: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct NpmPackageInfo {
  pub description: Option<String>,
  pub versions: HashMap<String, NpmVersionInfo>,
  pub time: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct NpmVersionInfo {
  pub dist: NpmDist,
}

#[derive(Debug, Deserialize)]
pub struct NpmDist {
  pub tarball: String,
  pub integrity: String,
}

pub async fn get_package_meta(
  client: &HttpClient,
  jsr_url: &Url,
  pkg: &JsrPackage,
) -> Result<PackageMeta, AnyError> {
  let url = jsr_url.join(&format!("@{}/{}/meta.json", pkg.scope, pkg.name))?;
  get_json(client, url).await
}

pub async fn get_latest_package_version(
  client: &HttpClient,
  jsr_url: &Url,
  pkg: &JsrPackage,
) -> Result<String, AnyError> {
  match get_package_meta(client, jsr_url, pkg).await?.latest {
    Some(latest) => Ok(latest),
    None => bail!("Unable to find latest version of {pkg}"),
  }
}

/// Fetches the package's metadata from the JSR npm registry, which is the
/// registry's host with an `npm.` prefix.
pub async fn get_npm_package_info(
  client: &HttpClient,
  jsr_url: &Url,
  pkg: &JsrPackage,
) -> Result<NpmPackageInfo, AnyError> {
  let mut url = jsr_url.clone();
  let host = match jsr_url.host_str() {
    Some(host) => format!("npm.{host}"),
    None => bail!("Invalid JSR URL {jsr_url}"),
  };
  url.set_host(Some(&host))?;
  url.set_path(&format!("@jsr/{}__{}", pkg.scope, pkg.name));
  get_json(client, url).await
}

async fn get_json<T: DeserializeOwned>(
  client: &HttpClient,
  url: Url,
) -> Result<T, AnyError> {
  let response = client.get(url.clone())?.send().await?;
  if !response.status().is_success() {
    bail!("Received {} from {}", response.status().as_u16(), url);
  }
  body_to_json(response.into_body()).await
}
