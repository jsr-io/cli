// Copyright 2018-2026 the Deno authors. MIT license.

use std::io::Write;

pub fn init(maybe_level: Option<log::Level>) {
  let log_level = maybe_level.unwrap_or(log::Level::Info);
  let logger = env_logger::Builder::from_env(
    env_logger::Env::new()
      // Use `JSR_LOG` and `JSR_LOG_STYLE` instead of `RUST_` prefix
      .filter_or("JSR_LOG", log_level.to_level_filter().to_string())
      .write_style("JSR_LOG_STYLE"),
  )
  .filter_module("globset", log::LevelFilter::Error)
  .filter_module("rustls", log::LevelFilter::Off)
  // swc_ecma_codegen's `srcmap!` macro emits error-level spans only on debug
  // build:
  // https://github.com/swc-project/swc/blob/74d6478be1eb8cdf1df096c360c159db64b64d8a/crates/swc_ecma_codegen/src/macros.rs#L112
  // We suppress them here to avoid flooding our CI logs in integration tests.
  .filter_module("swc_ecma_codegen", log::LevelFilter::Off)
  .filter_module("swc_common::source_map", log::LevelFilter::Off)
  .filter_module("swc_ecma_transforms_optimization", log::LevelFilter::Off)
  .filter_module("swc_ecma_parser", log::LevelFilter::Error)
  .filter_module("swc_ecma_lexer", log::LevelFilter::Error)
  // Suppress span lifecycle logs since they are too verbose
  .filter_module("tracing::span", log::LevelFilter::Off)
  .write_style(if deno_terminal::colors::use_color() {
    env_logger::WriteStyle::Always
  } else {
    env_logger::WriteStyle::Never
  })
  .format(|buf, record| {
    let mut target = record.target().to_string();
    if let Some(line_no) = record.line() {
      target.push(':');
      target.push_str(&line_no.to_string());
    }
    if record.level() <= log::Level::Info {
      // Print ERROR, WARN and INFO logs as they are
      writeln!(buf, "{}", record.args())
    } else {
      // Add prefix to DEBUG or TRACE logs
      writeln!(
        buf,
        "{} RS - {} - {}",
        record.level(),
        target,
        record.args()
      )
    }
  })
  .build();

  let max_level = logger.filter();
  let _ = log::set_boxed_logger(Box::new(logger));
  log::set_max_level(max_level);
}
