// Copyright the JSR authors. MIT license.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Error as AnyError;
use deno_terminal::colors;

use super::DependencyKind;
use super::JsrPackage;
use super::api;
use crate::http_util::HttpClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgManagerName {
  Npm,
  Yarn,
  Pnpm,
  Bun,
}

impl PkgManagerName {
  /// Reads the package manager from `npm_config_user_agent`, which package
  /// managers set for the commands they run, like `pnpm/9.0.0 npm/? ...`.
  fn from_user_agent(user_agent: &str) -> Option<Self> {
    [
      ("pnpm/", Self::Pnpm),
      ("yarn/", Self::Yarn),
      ("npm/", Self::Npm),
      ("bun/", Self::Bun),
    ]
    .into_iter()
    .find(|(prefix, _)| user_agent.starts_with(prefix))
    .map(|(_, name)| name)
  }
}

const LOCKFILES: [(&str, PkgManagerName); 5] = [
  ("package-lock.json", PkgManagerName::Npm),
  // bun allows a yarn.lock next to its own lockfile, so check bun's first,
  // see https://bun.sh/docs/install/lockfile
  ("bun.lockb", PkgManagerName::Bun),
  ("bun.lock", PkgManagerName::Bun),
  ("yarn.lock", PkgManagerName::Yarn),
  ("pnpm-lock.yaml", PkgManagerName::Pnpm),
];

#[derive(Debug, PartialEq, Eq)]
pub struct ProjectInfo {
  /// The directory of the closest package.json, or the cwd.
  pub project_dir: PathBuf,
  /// Detected from the closest lockfile.
  pub pkg_manager_name: Option<PkgManagerName>,
  pub pkg_json_path: Option<PathBuf>,
  /// The npm, yarn, bun or pnpm workspace root.
  pub root: Option<PathBuf>,
}

/// Walks up from `cwd` until it finds a lockfile.
pub fn find_project_dir(cwd: &Path) -> ProjectInfo {
  let mut result = ProjectInfo {
    project_dir: cwd.to_path_buf(),
    pkg_manager_name: None,
    pkg_json_path: None,
    root: None,
  };
  for dir in cwd.ancestors() {
    let pkg_json_path = dir.join("package.json");
    if pkg_json_path.is_file() {
      // the closest package.json defines the project
      if result.pkg_json_path.is_none() {
        log::debug!("Found package.json at {}", pkg_json_path.display());
        result.project_dir = dir.to_path_buf();
        result.pkg_json_path = Some(pkg_json_path);
      } else if has_workspaces(&pkg_json_path)
        || dir.join("pnpm-workspace.yaml").is_file()
      {
        result.root = Some(dir.to_path_buf());
      }
    }

    for (lockfile, name) in LOCKFILES {
      let lockfile = dir.join(lockfile);
      if lockfile.is_file() {
        log::debug!("Detected {name:?} from lockfile {}", lockfile.display());
        result.pkg_manager_name = Some(name);
        return result;
      }
    }
  }
  result
}

fn has_workspaces(pkg_json_path: &Path) -> bool {
  std::fs::read_to_string(pkg_json_path)
    .ok()
    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    .is_some_and(|json| json.get("workspaces").is_some_and(|w| w.is_array()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
  Npm,
  Yarn,
  YarnBerry,
  Pnpm,
  Bun,
}

pub struct PackageManager {
  kind: Kind,
  cwd: PathBuf,
}

impl PackageManager {
  /// Picks the package manager from, in order: `name`, the closest lockfile,
  /// the package manager running this command, or npm. Also returns the
  /// workspace root.
  pub fn detect(
    cwd: &Path,
    name: Option<PkgManagerName>,
  ) -> Result<(Self, PathBuf), AnyError> {
    let from_env = std::env::var("npm_config_user_agent")
      .ok()
      .and_then(|user_agent| PkgManagerName::from_user_agent(&user_agent));
    let project = find_project_dir(cwd);
    let root = project.root.unwrap_or_else(|| project.project_dir.clone());
    let cwd = project.project_dir;
    let kind = match name
      .or(project.pkg_manager_name)
      .or(from_env)
      .unwrap_or(PkgManagerName::Npm)
    {
      PkgManagerName::Npm => Kind::Npm,
      PkgManagerName::Yarn if is_yarn_berry(&cwd)? => Kind::YarnBerry,
      PkgManagerName::Yarn => Kind::Yarn,
      PkgManagerName::Pnpm => Kind::Pnpm,
      PkgManagerName::Bun => Kind::Bun,
    };
    Ok((Self { kind, cwd }, root))
  }

  pub fn is_yarn_berry(&self) -> bool {
    self.kind == Kind::YarnBerry
  }

  pub fn is_bun(&self) -> bool {
    self.kind == Kind::Bun
  }

  fn command(&self) -> &'static str {
    match self.kind {
      Kind::Npm => "npm",
      Kind::Yarn | Kind::YarnBerry => "yarn",
      Kind::Pnpm => "pnpm",
      Kind::Bun => "bun",
    }
  }

  pub async fn install(
    &self,
    client: &HttpClient,
    jsr_url: &url::Url,
    packages: &[JsrPackage],
    kind: DependencyKind,
  ) -> Result<(), AnyError> {
    let mut args = vec![
      match self.kind {
        Kind::Npm => "install",
        _ => "add",
      }
      .to_string(),
    ];
    let flag = match (self.kind, kind) {
      (_, DependencyKind::Prod) => None,
      (Kind::Npm | Kind::Pnpm, DependencyKind::Dev) => Some("--save-dev"),
      (Kind::Npm | Kind::Pnpm, DependencyKind::Optional) => {
        Some("--save-optional")
      }
      (_, DependencyKind::Dev) => Some("--dev"),
      (_, DependencyKind::Optional) => Some("--optional"),
    };
    args.extend(flag.map(String::from));
    for pkg in packages {
      let mut pkg = pkg.clone();
      if self.kind == Kind::YarnBerry && pkg.version.is_none() {
        // workaround for https://github.com/yarnpkg/berry/issues/1816
        let latest =
          api::get_latest_package_version(client, jsr_url, &pkg).await?;
        pkg.version = Some(format!("^{latest}"));
      }
      args.push(format!(
        "@{}/{}@npm:{}",
        pkg.scope,
        pkg.name,
        pkg.to_npm_package()
      ));
    }
    exec_with_log(self.command(), &args, &self.cwd)
  }

  pub fn remove(&self, packages: &[JsrPackage]) -> Result<(), AnyError> {
    let mut args = vec!["remove".to_string()];
    args.extend(packages.iter().map(|pkg| pkg.to_string()));
    exec_with_log(self.command(), &args, &self.cwd)
  }

  pub fn run_script(&self, script: &str) -> Result<(), AnyError> {
    let args: &[&str] = match self.kind {
      Kind::Npm | Kind::Bun => &["run", script],
      Kind::Yarn | Kind::YarnBerry | Kind::Pnpm => &[script],
    };
    exec_with_log(self.command(), args, &self.cwd)
  }

  /// Sets a yarn berry config value, see https://yarnpkg.com/cli/config/set.
  pub fn set_yarn_config_value(
    &self,
    key: &str,
    value: &str,
  ) -> Result<(), AnyError> {
    exec_with_log(self.command(), &["config", "set", key, value], &self.cwd)
  }

  /// Bun reads `.npmrc` since v1.1.18, see
  /// https://bun.sh/blog/bun-v1.1.18#npmrc-support
  pub fn is_bun_npmrc_supported(&self) -> Result<bool, AnyError> {
    let version = exec_output("bun", &["--version"], &self.cwd)?;
    let min_version = deno_semver::Version::parse_standard("1.1.18").unwrap();
    Ok(
      deno_semver::Version::parse_standard(&version)
        .is_ok_and(|version| version >= min_version),
    )
  }
}

fn is_yarn_berry(cwd: &Path) -> Result<bool, AnyError> {
  // works for both yarn classic and berry
  let version = exec_output("yarn", &["--version"], cwd)?;
  if version.is_empty() {
    log::debug!("Unable to detect yarn version, assuming classic");
    return Ok(false);
  }
  let is_berry = !version.starts_with("1.");
  log::debug!(
    "Detected yarn {} from version",
    if is_berry { "berry" } else { "classic" }
  );
  Ok(is_berry)
}

/// A command that exited with a non-zero exit code.
#[derive(Debug, thiserror::Error)]
#[error("`{command}` exited with code {code}")]
pub struct ExecError {
  pub command: String,
  pub code: i32,
}

fn resolve_command(cmd: &str) -> Result<PathBuf, AnyError> {
  // resolves `npm.cmd` and the like on Windows
  which::which(cmd).with_context(|| format!("Could not find `{cmd}`"))
}

fn exec_with_log<S: AsRef<str>>(
  cmd: &str,
  args: &[S],
  cwd: &Path,
) -> Result<(), AnyError> {
  let command_line = std::iter::once(cmd)
    .chain(args.iter().map(|arg| arg.as_ref()))
    .collect::<Vec<_>>()
    .join(" ");
  log::info!("{}", colors::gray(format!("$ {command_line}")));
  let status = Command::new(resolve_command(cmd)?)
    .args(args.iter().map(|arg| arg.as_ref()))
    .current_dir(cwd)
    .status()
    .with_context(|| format!("Failed to run `{command_line}`"))?;
  if status.success() {
    Ok(())
  } else {
    Err(
      ExecError {
        command: command_line,
        code: status.code().unwrap_or(1),
      }
      .into(),
    )
  }
}

fn exec_output(
  cmd: &str,
  args: &[&str],
  cwd: &Path,
) -> Result<String, AnyError> {
  let output = Command::new(resolve_command(cmd)?)
    .args(args)
    .current_dir(cwd)
    .output()
    .with_context(|| format!("Failed to run `{cmd}`"))?;
  if !output.status.success() {
    return Err(
      ExecError {
        command: format!("{cmd} {}", args.join(" ")),
        code: output.status.code().unwrap_or(1),
      }
      .into(),
    );
  }
  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod test {
  use super::*;

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir()
      .join(format!("jsr-cli-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
  }

  #[test]
  fn detects_package_manager_from_lockfile() {
    for (lockfiles, expected) in [
      (&["package-lock.json"][..], PkgManagerName::Npm),
      (&["yarn.lock"], PkgManagerName::Yarn),
      (&["pnpm-lock.yaml"], PkgManagerName::Pnpm),
      (&["bun.lockb"], PkgManagerName::Bun),
      (&["bun.lock"], PkgManagerName::Bun),
      (&["bun.lockb", "yarn.lock"], PkgManagerName::Bun),
    ] {
      let dir = temp_dir();
      for lockfile in lockfiles {
        std::fs::write(dir.join(lockfile), "").unwrap();
      }
      assert_eq!(find_project_dir(&dir).pkg_manager_name, Some(expected));
      std::fs::remove_dir_all(dir).unwrap();
    }
  }

  #[test]
  fn project_dir_is_closest_package_json() {
    let dir = temp_dir();
    let sub = dir.join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::write(sub.join("package.json"), "{}").unwrap();
    let result = find_project_dir(&sub);
    assert_eq!(result.project_dir, sub);
    assert_eq!(result.pkg_json_path, Some(sub.join("package.json")));
    assert_eq!(result.root, None);
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn finds_workspace_root() {
    for pnpm in [false, true] {
      let dir = temp_dir();
      let sub = dir.join("sub");
      std::fs::create_dir(&sub).unwrap();
      if pnpm {
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::write(dir.join("pnpm-workspace.yaml"), "").unwrap();
      } else {
        std::fs::write(dir.join("package.json"), r#"{"workspaces":["sub"]}"#)
          .unwrap();
      }
      std::fs::write(sub.join("package.json"), "{}").unwrap();
      assert_eq!(find_project_dir(&sub).root, Some(dir.clone()));
      std::fs::remove_dir_all(dir).unwrap();
    }
  }

  #[test]
  fn package_manager_from_user_agent() {
    assert_eq!(
      PkgManagerName::from_user_agent("pnpm/9.0.0 npm/? node/v22.0.0"),
      Some(PkgManagerName::Pnpm)
    );
    assert_eq!(
      PkgManagerName::from_user_agent("npm/10.0.0 node/v22.0.0"),
      Some(PkgManagerName::Npm)
    );
    assert_eq!(PkgManagerName::from_user_agent("deno/2.0.0"), None);
  }
}
