// Copyright 2018-2026 the Deno authors. MIT license.

//! Test utilities, based on `tests/util` in the Deno repository.

mod builders;
#[path = "server/npm.rs"]
mod npm;
pub mod server;
pub mod wildcard;

pub use builders::*;

pub mod consts {
  pub const JSR_REGISTRY_SERVER_PORT: u16 = 4250;
  pub const PROVENANCE_MOCK_SERVER_PORT: u16 = 4251;
  pub const PUBLIC_NPM_REGISTRY_PORT: u16 = 4260;
}

pub static IS_CI: std::sync::LazyLock<bool> =
  std::sync::LazyLock::new(|| std::env::var("CI").is_ok());

pub fn root_path() -> PathRef {
  PathRef::new(env!("CARGO_MANIFEST_DIR")).parent().parent()
}

pub fn tests_path() -> PathRef {
  root_path().join("tests")
}

pub fn testdata_path() -> PathRef {
  tests_path().join("testdata")
}

pub fn jsr_registry_url() -> String {
  format!("http://127.0.0.1:{}/", consts::JSR_REGISTRY_SERVER_PORT)
}

pub fn npm_registry_url() -> String {
  format!("http://localhost:{}/", consts::PUBLIC_NPM_REGISTRY_PORT)
}

pub fn rekor_url() -> String {
  format!("http://127.0.0.1:{}", consts::PROVENANCE_MOCK_SERVER_PORT)
}

pub fn fulcio_url() -> String {
  format!("http://127.0.0.1:{}", consts::PROVENANCE_MOCK_SERVER_PORT)
}

pub fn gha_token_url() -> String {
  format!(
    "http://127.0.0.1:{}/gha_oidc?test=true",
    consts::PROVENANCE_MOCK_SERVER_PORT
  )
}

pub fn env_vars_for_npm_tests() -> Vec<(String, String)> {
  vec![
    ("NPM_CONFIG_REGISTRY".to_string(), npm_registry_url()),
    ("NPM_CONFIG_MIN_RELEASE_AGE".to_string(), "0".to_string()),
    ("NO_COLOR".to_string(), "1".to_string()),
  ]
}

pub fn env_vars_for_jsr_tests_with_git_check() -> Vec<(String, String)> {
  vec![
    ("JSR_URL".to_string(), jsr_registry_url()),
    ("DISABLE_JSR_PROVENANCE".to_string(), "true".to_string()),
    ("NO_COLOR".to_string(), "1".to_string()),
  ]
}

pub fn env_vars_for_jsr_tests() -> Vec<(String, String)> {
  let mut vars = env_vars_for_jsr_tests_with_git_check();

  vars.push((
    "DENO_TESTING_DISABLE_GIT_CHECK".to_string(),
    "1".to_string(),
  ));

  vars
}

pub fn env_vars_for_jsr_provenance_tests() -> Vec<(String, String)> {
  let mut envs = env_vars_for_jsr_tests();
  envs.retain(|(key, _)| key != "DISABLE_JSR_PROVENANCE");
  envs.extend(vec![
    ("REKOR_URL".to_string(), rekor_url()),
    ("FULCIO_URL".to_string(), fulcio_url()),
    (
      "DISABLE_JSR_MANIFEST_VERIFICATION_FOR_TESTING".to_string(),
      "true".to_string(),
    ),
  ]);
  // set GHA variable for attestation.
  envs.extend([
    ("CI".to_string(), "true".to_string()),
    ("GITHUB_ACTIONS".to_string(), "true".to_string()),
    ("ACTIONS_ID_TOKEN_REQUEST_URL".to_string(), gha_token_url()),
    (
      "ACTIONS_ID_TOKEN_REQUEST_TOKEN".to_string(),
      "dummy".to_string(),
    ),
    (
      "GITHUB_REPOSITORY".to_string(),
      "littledivy/deno_sdl2".to_string(),
    ),
    (
      "GITHUB_SERVER_URL".to_string(),
      "https://github.com".to_string(),
    ),
    ("GITHUB_REF".to_string(), "refs/tags/sdl2@0.0.1".to_string()),
    ("GITHUB_SHA".to_string(), "lol".to_string()),
    ("GITHUB_RUN_ID".to_string(), "1".to_string()),
    ("GITHUB_RUN_ATTEMPT".to_string(), "1".to_string()),
    (
      "RUNNER_ENVIRONMENT".to_string(),
      "github-hosted".to_string(),
    ),
    (
      "GITHUB_WORKFLOW_REF".to_string(),
      "littledivy/deno_sdl2@refs/tags/sdl2@0.0.1".to_string(),
    ),
  ]);

  envs
}

#[macro_export]
macro_rules! println {
  ($($arg:tt)*) => {
    ::std::println!($($arg)*)
  };
}

#[macro_export]
macro_rules! eprintln {
  ($($arg:tt)*) => {
    ::std::eprintln!($($arg)*)
  };
}

#[macro_export]
macro_rules! assert_not_contains {
  ($string:expr, $($test:expr),+ $(,)?) => {
    let string = &$string; // This might be a function call or something
    if !($(!string.contains($test))||+) {
      panic!("{:?} contained {:?}", string, [$($test),+]);
    }
  }
}

#[macro_export]
macro_rules! assert_contains {
  ($string:expr, $($test:expr),+ $(,)?) => {
    let string = &$string; // This might be a function call or something
    if !($(string.contains($test))||+) {
      panic!("{:?} does not contain any of {:?}", string, [$($test),+]);
    }
  }
}

pub(crate) mod colors {
  use std::io::Write;

  use termcolor::Ansi;
  use termcolor::Color;
  use termcolor::ColorSpec;
  use termcolor::WriteColor;

  pub fn red<S: AsRef<str>>(s: S) -> String {
    fg_color(s, Color::Red)
  }

  pub fn green<S: AsRef<str>>(s: S) -> String {
    fg_color(s, Color::Green)
  }

  pub fn gray<S: AsRef<str>>(s: S) -> String {
    fg_color(s, Color::Ansi256(245))
  }

  fn fg_color<S: AsRef<str>>(s: S, color: Color) -> String {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(color));
    style(s, style_spec)
  }

  fn style<S: AsRef<str>>(s: S, colorspec: ColorSpec) -> String {
    let mut v = Vec::new();
    let mut ansi_writer = Ansi::new(&mut v);
    ansi_writer.set_color(&colorspec).unwrap();
    ansi_writer.write_all(s.as_ref().as_bytes()).unwrap();
    ansi_writer.reset().unwrap();
    String::from_utf8_lossy(&v).into_owned()
  }
}
