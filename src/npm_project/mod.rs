// Copyright the JSR authors. MIT license.

//! Managing the JSR dependencies of npm, yarn, pnpm and bun projects, ported
//! from https://github.com/jsr-io/jsr-npm.

mod api;
mod jsr_package;
mod package_manager;

use std::path::Path;

use anyhow::Context;
use anyhow::Error as AnyError;
use anyhow::bail;
use deno_terminal::colors;

pub use jsr_package::JsrPackage;
pub use jsr_package::JsrPackageNameError;
pub use package_manager::ExecError;
use package_manager::PackageManager;
pub use package_manager::PkgManagerName;

use crate::http_util::HttpClientProvider;

const NPMRC_FILE: &str = ".npmrc";
const BUNFIG_FILE: &str = "bunfig.toml";
const JSR_NPM_REGISTRY_URL: &str = "https://npm.jsr.io";
const JSR_YARN_BERRY_CONFIG_KEY: &str = "npmScopes.jsr.npmRegistryServer";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
  Prod,
  Dev,
  Optional,
}

/// Adds the packages to the package.json and installs them. Without packages,
/// installs the existing dependencies.
pub async fn add(
  cwd: &Path,
  packages: &[String],
  kind: DependencyKind,
  pkg_manager_name: Option<PkgManagerName>,
) -> Result<(), AnyError> {
  let packages = parse_packages(packages)?;
  let (pkg_manager, root) = PackageManager::detect(cwd, pkg_manager_name)?;

  if !packages.is_empty() {
    if pkg_manager.is_bun() && !pkg_manager.is_bun_npmrc_supported()? {
      setup_bunfig_toml(&root)?;
    } else if pkg_manager.is_yarn_berry() {
      // yarn v2+ intentionally doesn't read .npmrc, see
      // https://yarnpkg.com/migration/guide#update-your-configuration-to-the-new-settings
      pkg_manager.set_yarn_config_value(
        JSR_YARN_BERRY_CONFIG_KEY,
        JSR_NPM_REGISTRY_URL,
      )?;
    } else {
      setup_npmrc(&root)?;
    }
    log::info!("Installing {}...", colors::cyan(join(&packages)));
  }

  let client = HttpClientProvider::default().get_or_create()?;
  let jsr_url = jsr_url();
  pkg_manager
    .install(&client, &jsr_url, &packages, kind)
    .await
}

/// Removes the packages from the package.json.
pub fn remove(
  cwd: &Path,
  packages: &[String],
  pkg_manager_name: Option<PkgManagerName>,
) -> Result<(), AnyError> {
  let packages = parse_packages(packages)?;
  let (pkg_manager, _) = PackageManager::detect(cwd, pkg_manager_name)?;
  log::info!("Removing {}...", colors::cyan(join(&packages)));
  pkg_manager.remove(&packages)
}

/// Runs a script from the package.json.
pub fn run_script(
  cwd: &Path,
  script: &str,
  pkg_manager_name: Option<PkgManagerName>,
) -> Result<(), AnyError> {
  let (pkg_manager, _) = PackageManager::detect(cwd, pkg_manager_name)?;
  pkg_manager.run_script(script)
}

/// Whether the package.json in `cwd` has the script.
pub fn has_script(cwd: &Path, script: &str) -> bool {
  std::fs::read_to_string(cwd.join("package.json"))
    .ok()
    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    .is_some_and(|json| {
      json
        .get("scripts")
        .and_then(|scripts| scripts.get(script))
        .is_some()
    })
}

/// Prints information about a package and its npm tarball.
pub async fn info(package: &str) -> Result<(), AnyError> {
  let pkg = JsrPackage::parse(package)?;
  let client = HttpClientProvider::default().get_or_create()?;
  let jsr_url = jsr_url();

  let meta = api::get_package_meta(&client, &jsr_url, &pkg).await?;
  let version = match &pkg.version {
    Some(version) => version.clone(),
    // without a stable version, show the newest pre-release
    None => match meta.latest.clone().or_else(|| {
      meta
        .versions
        .keys()
        .filter_map(|v| deno_semver::Version::parse_standard(v).ok())
        .max()
        .map(|v| v.to_string())
    }) {
      Some(version) => version,
      None => bail!("Could not find published version for {pkg}"),
    },
  };

  let npm_info = api::get_npm_package_info(&client, &jsr_url, &pkg).await?;
  let version_info = npm_info
    .versions
    .get(&version)
    .with_context(|| format!("Could not find version {version} of {pkg}"))?;

  println!();
  println!(
    "{} | latest: {} | versions: {}",
    colors::cyan(format!("@{}/{}@{}", pkg.scope, pkg.name, version)),
    colors::magenta(meta.latest.as_deref().unwrap_or("-")),
    colors::magenta(meta.versions.len()),
  );
  println!("{}", npm_info.description.as_deref().unwrap_or_default());
  println!();
  println!(
    "npm tarball:   {}",
    colors::cyan(&version_info.dist.tarball)
  );
  println!(
    "npm integrity: {}",
    colors::cyan(&version_info.dist.integrity)
  );
  if let Some(published) = npm_info
    .time
    .get(&version)
    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
  {
    let now_ms = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_millis() as i64)
      .unwrap_or_default();
    println!();
    println!(
      "published: {}",
      colors::magenta(time_ago(now_ms - published.timestamp_millis()))
    );
  }
  Ok(())
}

fn jsr_url() -> url::Url {
  deno_resolver::factory::resolve_jsr_url(&crate::Sys::default())
}

fn parse_packages(packages: &[String]) -> Result<Vec<JsrPackage>, AnyError> {
  packages
    .iter()
    .map(|pkg| JsrPackage::parse(pkg).map_err(AnyError::from))
    .collect()
}

fn join(packages: &[JsrPackage]) -> String {
  packages
    .iter()
    .map(|pkg| pkg.to_string())
    .collect::<Vec<_>>()
    .join(", ")
}

/// Points the `@jsr` scope at the JSR npm registry in the `.npmrc`.
fn setup_npmrc(dir: &Path) -> Result<(), AnyError> {
  let path = dir.join(NPMRC_FILE);
  let jsr_npmrc = format!("@jsr:registry={JSR_NPM_REGISTRY_URL}\n");
  let content = match std::fs::read_to_string(&path) {
    Ok(content) if content.contains("@jsr:registry=") => return Ok(()),
    Ok(content) => {
      let newline = if content.contains("\r\n") {
        "\r\n"
      } else {
        "\n"
      };
      let spacer = if content.is_empty() || content.ends_with(newline) {
        ""
      } else {
        newline
      };
      format!("{content}{spacer}{jsr_npmrc}")
    }
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => jsr_npmrc,
    Err(err) => return Err(err.into()),
  };
  std::fs::write(&path, content)?;
  log::info!("Setting up {NPMRC_FILE}...{}", colors::green("ok"));
  Ok(())
}

/// Points the `@jsr` scope at the JSR npm registry in the `bunfig.toml`, for
/// bun versions that don't read `.npmrc`.
fn setup_bunfig_toml(dir: &Path) -> Result<(), AnyError> {
  let path = dir.join(BUNFIG_FILE);
  let jsr_bunfig =
    format!("[install.scopes]\n\"@jsr\" = \"{JSR_NPM_REGISTRY_URL}\"\n");
  let content = match std::fs::read_to_string(&path) {
    Ok(content)
      if lazy_regex::regex_is_match!(r#"(?m)^"@jsr"\s+="#, &content) =>
    {
      return Ok(());
    }
    Ok(content) => format!("{content}{jsr_bunfig}"),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => jsr_bunfig,
    Err(err) => return Err(err.into()),
  };
  std::fs::write(&path, content)?;
  log::info!("Setting up {BUNFIG_FILE}...{}", colors::green("ok"));
  Ok(())
}

fn time_ago(diff_ms: i64) -> String {
  const SECOND: i64 = 1000;
  const MINUTE: i64 = 60 * SECOND;
  const HOUR: i64 = 60 * MINUTE;
  const DAY: i64 = 24 * HOUR;
  const WEEK: i64 = 7 * DAY;
  const MONTH: i64 = 30 * DAY;
  const YEAR: i64 = 365 * DAY;
  for (period, unit) in [
    (YEAR, "year"),
    (MONTH, "month"),
    (WEEK, "week"),
    (DAY, "day"),
    (HOUR, "hour"),
    (MINUTE, "minute"),
    (SECOND, "second"),
  ] {
    if diff_ms > period {
      let value = diff_ms / period;
      let plural = if value > 1 { "s" } else { "" };
      return format!("{value} {unit}{plural} ago");
    }
  }
  "just now".to_string()
}

#[cfg(test)]
mod test {
  use super::*;

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir()
      .join(format!("jsr-cli-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn npmrc_created() {
    let dir = temp_dir();
    setup_npmrc(&dir).unwrap();
    assert_eq!(
      std::fs::read_to_string(dir.join(".npmrc")).unwrap(),
      "@jsr:registry=https://npm.jsr.io\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn npmrc_keeps_existing_jsr_mapping() {
    let dir = temp_dir();
    let npmrc = dir.join(".npmrc");
    std::fs::write(&npmrc, "@jsr:registry=https://example.com\n").unwrap();
    setup_npmrc(&dir).unwrap();
    assert_eq!(
      std::fs::read_to_string(&npmrc).unwrap(),
      "@jsr:registry=https://example.com\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn npmrc_adds_newline_between_entries() {
    let dir = temp_dir();
    let npmrc = dir.join(".npmrc");
    for (existing, expected) in [
      (
        "@foo:registry=https://example.com",
        "@foo:registry=https://example.com\n@jsr:registry=https://npm.jsr.io\n",
      ),
      (
        "@foo:registry=https://example.com\r\n",
        "@foo:registry=https://example.com\r\n@jsr:registry=https://npm.jsr.io\n",
      ),
    ] {
      std::fs::write(&npmrc, existing).unwrap();
      setup_npmrc(&dir).unwrap();
      assert_eq!(std::fs::read_to_string(&npmrc).unwrap(), expected);
    }
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn bunfig_keeps_existing_jsr_scope() {
    let dir = temp_dir();
    let bunfig = dir.join("bunfig.toml");
    let existing = "[install.scopes]\n\"@jsr\" = \"https://example.com\"\n";
    std::fs::write(&bunfig, existing).unwrap();
    setup_bunfig_toml(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(&bunfig).unwrap(), existing);
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn time_ago_formats() {
    assert_eq!(time_ago(500), "just now");
    assert_eq!(time_ago(5_000), "5 seconds ago");
    assert_eq!(time_ago(61_000), "1 minute ago");
    assert_eq!(time_ago(3 * 24 * 60 * 60 * 1000 + 1), "3 days ago");
  }
}
