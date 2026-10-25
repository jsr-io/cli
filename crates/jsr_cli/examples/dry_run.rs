// Copyright 2018-2026 the Deno authors. MIT license.

//! Runs `publish --dry-run` for the package in the given directory.
//!
//! cargo run --example dry_run -- <dir> [--allow-slow-types]

#![allow(clippy::print_stderr, reason = "example")]

use std::path::PathBuf;

use jsr_cli::publish::PublishOptions;

struct StderrLogger;

impl log::Log for StderrLogger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= log::Level::Info
  }

  fn log(&self, record: &log::Record) {
    if self.enabled(record.metadata()) {
      eprintln!("{}", record.args());
    }
  }

  fn flush(&self) {}
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
  log::set_logger(&StderrLogger).unwrap();
  log::set_max_level(log::LevelFilter::Info);

  let mut args = std::env::args().skip(1);
  let cwd = PathBuf::from(args.next().expect("expected a directory"));
  let allow_slow_types = args.any(|a| a == "--allow-slow-types");
  let result = jsr_cli::publish::publish(PublishOptions {
    cwd: std::fs::canonicalize(cwd).unwrap(),
    dry_run: true,
    allow_dirty: true,
    allow_slow_types,
    ..Default::default()
  })
  .await;
  if let Err(err) = result {
    eprintln!("error: {err:#}");
    std::process::exit(1);
  }
}
