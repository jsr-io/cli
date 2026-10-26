// Copyright 2018-2026 the Deno authors. MIT license.

//! A small subset of the `TestContextBuilder` and `TestCommandBuilder` API
//! from `tests/util/lib/builders.rs` in the Deno repository, enough to run
//! the publish and pack tests.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use crate::env_vars_for_jsr_tests;
use crate::env_vars_for_npm_tests;
use crate::testdata_path;
use crate::tests_path;
use crate::wildcard::WildcardMatchResult;
use crate::wildcard::wildcard_match_detailed;

/// A path, with the helpers the tests use.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathRef(PathBuf);

impl PathRef {
  pub fn new(path: impl AsRef<Path>) -> Self {
    Self(path.as_ref().to_path_buf())
  }

  pub fn parent(&self) -> PathRef {
    PathRef(self.0.parent().unwrap().to_path_buf())
  }

  pub fn join(&self, path: impl AsRef<Path>) -> PathRef {
    PathRef(self.0.join(path))
  }

  pub fn as_path(&self) -> &Path {
    &self.0
  }

  pub fn to_path_buf(&self) -> PathBuf {
    self.0.clone()
  }

  pub fn to_string_lossy(&self) -> std::borrow::Cow<'_, str> {
    self.0.to_string_lossy()
  }

  pub fn exists(&self) -> bool {
    self.0.exists()
  }

  pub fn create_dir_all(&self) {
    std::fs::create_dir_all(&self.0).unwrap();
  }

  pub fn write(&self, text: impl AsRef<str>) {
    std::fs::write(&self.0, text.as_ref()).unwrap();
  }

  pub fn write_json<TValue: serde::Serialize>(&self, value: &TValue) {
    let text = serde_json::to_string_pretty(value).unwrap();
    self.write(text);
  }

  pub fn read_to_string(&self) -> String {
    std::fs::read_to_string(&self.0)
      .unwrap_or_else(|err| panic!("failed reading {}: {err}", self))
  }

  pub fn read_jsonc_value(&self) -> serde_json::Value {
    jsonc_parser::parse_to_serde_value::<serde_json::Value>(
      &self.read_to_string(),
      &Default::default(),
    )
    .unwrap_or_else(|err| panic!("Failed to parse {}: {err}", self))
  }

  pub fn copy_to_recursive_with_exclusions(
    &self,
    to: &PathRef,
    exclusions: &HashSet<PathRef>,
  ) {
    to.create_dir_all();
    for entry in std::fs::read_dir(&self.0).unwrap() {
      let entry = entry.unwrap();
      let path = PathRef(entry.path());
      if exclusions.contains(&path) {
        continue;
      }
      let target = to.join(entry.file_name());
      let file_type = entry.file_type().unwrap();
      if file_type.is_dir() {
        path.copy_to_recursive_with_exclusions(&target, exclusions);
      } else if file_type.is_symlink() {
        let link = std::fs::read_link(&path.0).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(link, &target.0).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(link, &target.0).unwrap();
      } else {
        std::fs::copy(&path.0, &target.0).unwrap();
      }
    }
  }
}

impl AsRef<Path> for PathRef {
  fn as_ref(&self) -> &Path {
    &self.0
  }
}

impl fmt::Display for PathRef {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.0.display())
  }
}

pub struct TempDir {
  _inner: tempfile::TempDir,
  path: PathRef,
}

impl TempDir {
  fn new() -> Self {
    let inner = tempfile::TempDir::new().unwrap();
    // canonicalize because the temp dir is a symlink on macOS
    let path = PathRef::new(std::fs::canonicalize(inner.path()).unwrap());
    Self {
      _inner: inner,
      path,
    }
  }

  pub fn path(&self) -> &PathRef {
    &self.path
  }
}

#[derive(Default)]
pub struct TestContextBuilder {
  use_temp_cwd: bool,
  cwd: Option<String>,
  envs: HashMap<String, String>,
}

impl TestContextBuilder {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn use_http_server(self) -> Self {
    crate::server::start_servers();
    self
  }

  pub fn use_temp_cwd(mut self) -> Self {
    self.use_temp_cwd = true;
    self
  }

  /// Sets the current directory, relative to `tests/testdata`.
  pub fn cwd(mut self, cwd: impl AsRef<str>) -> Self {
    self.cwd = Some(cwd.as_ref().to_string());
    self
  }

  pub fn envs<I, K, V>(mut self, envs: I) -> Self
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
  {
    for (key, value) in envs {
      self
        .envs
        .insert(key.as_ref().to_string(), value.as_ref().to_string());
    }
    self
  }

  pub fn add_jsr_env_vars(self) -> Self {
    self.use_http_server().envs(env_vars_for_jsr_tests())
  }

  pub fn add_npm_env_vars(self) -> Self {
    self.use_http_server().envs(env_vars_for_npm_tests())
  }

  pub fn build(self) -> TestContext {
    let temp_dir = TempDir::new();
    let deno_dir = TempDir::new();
    let cwd = if self.use_temp_cwd {
      temp_dir.path().clone()
    } else {
      match &self.cwd {
        Some(cwd) => testdata_path().join(cwd),
        None => testdata_path(),
      }
    };
    TestContext {
      cwd,
      temp_dir,
      deno_dir,
      envs: self.envs,
    }
  }
}

pub struct TestContext {
  cwd: PathRef,
  temp_dir: TempDir,
  deno_dir: TempDir,
  envs: HashMap<String, String>,
}

impl TestContext {
  pub fn temp_dir(&self) -> &TempDir {
    &self.temp_dir
  }

  pub fn new_command(&self) -> TestCommandBuilder<'_> {
    TestCommandBuilder {
      context: self,
      command_name: None,
      args_text: String::new(),
      args_vec: Vec::new(),
      cwd: None,
      envs: self.envs.clone(),
      stdin_text: None,
    }
  }
}

pub struct TestCommandBuilder<'a> {
  context: &'a TestContext,
  command_name: Option<String>,
  args_text: String,
  args_vec: Vec<String>,
  cwd: Option<String>,
  envs: HashMap<String, String>,
  stdin_text: Option<String>,
}

impl TestCommandBuilder<'_> {
  pub fn name(mut self, name: impl AsRef<str>) -> Self {
    self.command_name = Some(name.as_ref().to_string());
    self
  }

  pub fn args(mut self, args: impl AsRef<str>) -> Self {
    self.args_text = args.as_ref().to_string();
    self
  }

  pub fn arg(mut self, arg: impl AsRef<str>) -> Self {
    self.args_vec.push(arg.as_ref().to_string());
    self
  }

  pub fn args_vec<I, S>(mut self, args: I) -> Self
  where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
  {
    self
      .args_vec
      .extend(args.into_iter().map(|s| s.as_ref().to_string()));
    self
  }

  pub fn current_dir(mut self, cwd: impl AsRef<str>) -> Self {
    self.cwd = Some(cwd.as_ref().to_string());
    self
  }

  pub fn envs<I, K, V>(mut self, envs: I) -> Self
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
  {
    for (key, value) in envs {
      self
        .envs
        .insert(key.as_ref().to_string(), value.as_ref().to_string());
    }
    self
  }

  pub fn stdin_text(mut self, text: impl AsRef<str>) -> Self {
    self.stdin_text = Some(text.as_ref().to_string());
    self
  }

  pub fn run(&self) -> TestCommandOutput {
    let cwd = match &self.cwd {
      Some(cwd) => self.context.cwd.join(cwd),
      None => self.context.cwd.clone(),
    };
    let args = if self.args_vec.is_empty() {
      self
        .args_text
        .split_whitespace()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
    } else {
      self.args_vec.clone()
    }
    .iter()
    .map(|arg| self.replace_vars(arg, &cwd))
    .collect::<Vec<_>>();

    let program = match &self.command_name {
      Some(name) => name.clone(),
      // `deno publish` and `deno pack` are run with the jsr binary. Other
      // deno subcommands used to set up a test (ex. `deno install`) use the
      // deno on the PATH.
      None => match args.first().map(|s| s.as_str()) {
        Some("publish" | "pack") => {
          jsr_exe_path().to_string_lossy().into_owned()
        }
        _ => "deno".to_string(),
      },
    };

    let mut envs = self.envs.clone();
    for value in envs.values_mut() {
      *value = self.replace_vars(value, &cwd);
    }
    envs
      .entry("DENO_DIR".to_string())
      .or_insert_with(|| self.context.deno_dir.path().to_string_lossy().into());
    // for scripts that spawn the jsr binary themselves
    envs
      .entry("JSR_BIN".to_string())
      .or_insert_with(|| jsr_exe_path().to_string_lossy().into_owned());

    let (mut reader, writer) = std::io::pipe().unwrap();
    let mut command = Command::new(&program);
    command
      .args(&args)
      .current_dir(&cwd)
      .envs(envs)
      .stdout(writer.try_clone().unwrap())
      .stderr(writer)
      .stdin(if self.stdin_text.is_some() {
        Stdio::piped()
      } else {
        Stdio::null()
      });
    let command_text = format!("{} {}", program, args.join(" "));
    let mut child = command
      .spawn()
      .unwrap_or_else(|err| panic!("failed to run `{command_text}`: {err}"));
    // drop the write ends held by `command` so reading reaches EOF
    drop(command);
    if let Some(text) = &self.stdin_text {
      let mut stdin = child.stdin.take().unwrap();
      stdin.write_all(text.as_bytes()).unwrap();
    }
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    let status = child.wait().unwrap();

    TestCommandOutput {
      command_text,
      // like Deno's test builders, compare the output without colors
      combined: console_static_text::ansi::strip_ansi_codes(&output)
        .into_owned(),
      exit_code: status.code(),
    }
  }

  fn replace_vars(&self, text: &str, cwd: &PathRef) -> String {
    text
      .replace("$DENO_DIR", &self.context.deno_dir.path().to_string_lossy())
      .replace("$TESTDATA", &testdata_path().to_string_lossy())
      .replace("$TESTS", &tests_path().to_string_lossy())
      .replace("$PWD", &cwd.to_string_lossy())
  }
}

pub fn jsr_exe_path() -> PathBuf {
  if let Ok(path) = std::env::var("JSR_TEST_EXE") {
    return PathBuf::from(path);
  }
  // target/<profile>/deps/<test exe> -> target/<profile>/jsr
  let mut path = std::env::current_exe().unwrap();
  path.pop();
  if path.ends_with("deps") {
    path.pop();
  }
  path.join(if cfg!(windows) { "jsr.exe" } else { "jsr" })
}

pub struct TestCommandOutput {
  command_text: String,
  combined: String,
  exit_code: Option<i32>,
}

impl TestCommandOutput {
  pub fn combined_output(&self) -> &str {
    &self.combined
  }

  #[track_caller]
  pub fn assert_matches_text(&self, expected_text: impl AsRef<str>) -> &Self {
    self.assert_wildcard_match(expected_text.as_ref());
    self
  }

  /// Asserts that the output matches the file, relative to `tests/testdata`.
  #[track_caller]
  pub fn assert_matches_file(&self, file_path: impl AsRef<Path>) -> &Self {
    let file_path = testdata_path().join(file_path);
    let expected = file_path.read_to_string();
    self.assert_wildcard_match(&expected);
    self
  }

  #[track_caller]
  pub fn assert_exit_code(&self, expected_exit_code: i32) -> &Self {
    if self.exit_code != Some(expected_exit_code) {
      panic!(
        "bad exit code for `{}`, expected: {:?}, actual: {:?}\n\n-- OUTPUT --\n{}",
        self.command_text, expected_exit_code, self.exit_code, self.combined
      );
    }
    self
  }

  #[track_caller]
  fn assert_wildcard_match(&self, expected: &str) {
    let actual = self.combined.replace("\r\n", "\n");
    if !expected.contains("[WILD")
      && !expected.contains("[UNORDERED_START]")
      && !expected.contains("[#")
    {
      pretty_assertions::assert_eq!(
        actual,
        expected,
        "output of `{}`",
        self.command_text
      );
    } else if let WildcardMatchResult::Fail(debug_output) =
      wildcard_match_detailed(expected, &actual)
    {
      panic!(
        "output of `{}` didn't match\n\n-- OUTPUT --\n{}\n-- EXPECTED --\n{}\n-- DEBUG --\n{}",
        self.command_text, actual, expected, debug_output
      );
    }
  }
}
