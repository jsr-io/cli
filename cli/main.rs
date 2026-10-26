// Copyright 2018-2026 the Deno authors. MIT license.

mod logger;

use std::path::PathBuf;

use clap::Args;
use clap::Parser;
use clap::Subcommand;
use deno_terminal::colors;
use jsr_cli::FactoryOptions;
use jsr_cli::pack::FileFlags;
use jsr_cli::pack::PackFlags;
use jsr_cli::publish::PublishOptions;

/// The JSR command line tool.
#[derive(Parser)]
#[command(name = "jsr", version)]
struct Cli {
  /// Set log level
  #[arg(
    short = 'L',
    long,
    global = true,
    value_parser = ["trace", "debug", "info"],
  )]
  log_level: Option<String>,

  /// Suppress diagnostic output
  #[arg(short, long, global = true)]
  quiet: bool,

  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Publish the current working directory's package or workspace
  Publish(PublishArgs),
  /// Create a tarball of the package
  Pack(PackArgs),
}

/// Config file and unstable feature arguments shared by the subcommands.
#[derive(Args)]
struct ConfigArgs {
  /// Configure different aspects of deno including TypeScript, linting, and
  /// code formatting.
  /// Typically the configuration file will be called `deno.json` or
  /// `deno.jsonc` and automatically detected; in that case this flag is not
  /// necessary.
  #[arg(short, long, value_name = "FILE")]
  config: Option<PathBuf>,

  /// Disable automatic loading of the configuration file
  #[arg(long, conflicts_with = "config")]
  no_config: bool,

  /// Enable unstable resolving of specifiers by extension probing, .js to
  /// .ts, and directory probing
  #[arg(long, alias = "sloppy-imports")]
  unstable_sloppy_imports: bool,

  /// Enable unstable 'bytes' and 'text' imports
  #[arg(long)]
  unstable_raw_imports: bool,

  /// Accepted for compatibility with Deno, where it doesn't affect
  /// publishing. The jsr npm package passes it.
  #[arg(long, hide = true)]
  unstable_bare_node_builtins: bool,

  /// Accepted for compatibility with Deno, where it doesn't affect
  /// publishing. The jsr npm package passes it.
  #[arg(long, hide = true)]
  unstable_byonm: bool,
}

impl ConfigArgs {
  fn into_factory_options(self) -> Result<FactoryOptions, anyhow::Error> {
    Ok(FactoryOptions {
      cwd: std::env::current_dir()?,
      config: self.config,
      no_config: self.no_config,
      sloppy_imports: self.unstable_sloppy_imports,
      unstable_raw_imports: self.unstable_raw_imports,
    })
  }
}

/// Arguments of `deno pack`, see `PACK_SUBCOMMAND` in
/// `libs/cli_parser/src/defs.rs` in the Deno repository.
#[derive(Args)]
struct PackArgs {
  /// List of file patterns to include
  files: Vec<String>,

  /// Output file path (defaults to <name>-<version>.tgz)
  #[arg(short, long)]
  output: Option<String>,

  /// Show what would be packed without creating the tarball
  #[arg(long)]
  dry_run: bool,

  /// Skip fast-check type extraction; .d.ts files are omitted from the
  /// output
  #[arg(long)]
  allow_slow_types: bool,

  /// Allow packing if the repository has uncommitted changes
  #[arg(long)]
  allow_dirty: bool,

  /// Override the version in the tarball
  #[arg(long)]
  set_version: Option<String>,

  /// Don't include source maps in the output
  #[arg(long)]
  no_source_maps: bool,

  /// Ignore files matching these patterns
  #[arg(long, num_args = 0.., require_equals = true, value_delimiter = ',')]
  ignore: Vec<String>,

  #[command(flatten)]
  config: ConfigArgs,
}

/// Arguments of `deno publish`, see `PUBLISH_SUBCOMMAND` in
/// `libs/cli_parser/src/defs.rs` in the Deno repository.
#[derive(Args)]
struct PublishArgs {
  /// The API token to use when publishing. If unset, interactive
  /// authentication is be used
  #[arg(long)]
  token: Option<String>,

  /// Prepare the package for publishing performing all checks and
  /// validations without uploading
  #[arg(long)]
  dry_run: bool,

  /// Allow publishing with slow types
  #[arg(long)]
  allow_slow_types: bool,

  /// Allow publishing if the repository has uncommitted changed
  #[arg(long)]
  allow_dirty: bool,

  /// Disable provenance attestation.
  /// Enabled by default on Github actions, publicly links the package to
  /// where it was built and published from.
  #[arg(long)]
  no_provenance: bool,

  /// Set version for a package to be published.
  /// This flag can be used while publishing individual packages and cannot
  /// be used in a workspace.
  #[arg(long)]
  set_version: Option<String>,

  /// Accepted for compatibility with `deno publish`. This tool doesn't type
  /// check.
  #[arg(long, num_args = 0..=1, require_equals = true, value_name = "CHECK_TYPE")]
  check: Option<Option<String>>,

  /// Accepted for compatibility with `deno publish`. This tool doesn't type
  /// check.
  #[arg(long, num_args = 0..=1, require_equals = true, value_name = "NO_CHECK_TYPE")]
  no_check: Option<Option<String>>,

  #[command(flatten)]
  config: ConfigArgs,
}

fn main() {
  let cli = Cli::parse();
  logger::init(if cli.quiet {
    Some(log::Level::Error)
  } else {
    cli.log_level.as_deref().map(|level| match level {
      "trace" => log::Level::Trace,
      "debug" => log::Level::Debug,
      _ => log::Level::Info,
    })
  });

  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  let result = runtime.block_on(run(cli.command));
  if let Err(err) = result {
    log::error!(
      "{}: {}",
      colors::red_bold("error"),
      format!("{err:?}").trim_start_matches("error: ")
    );
    std::process::exit(1);
  }
}

async fn run(command: Command) -> Result<(), anyhow::Error> {
  match command {
    Command::Publish(args) => {
      if args.check.is_some() {
        log::warn!(
          "{} jsr publish doesn't type check. Run `deno check` or `tsc` instead.",
          colors::yellow("Warning"),
        );
      }
      let config = args.config.into_factory_options()?;
      jsr_cli::publish::publish(PublishOptions {
        cwd: config.cwd,
        config: config.config,
        no_config: config.no_config,
        token: args.token,
        dry_run: args.dry_run,
        allow_slow_types: args.allow_slow_types,
        allow_dirty: args.allow_dirty,
        no_provenance: args.no_provenance,
        set_version: args.set_version,
        sloppy_imports: config.sloppy_imports,
        unstable_raw_imports: config.unstable_raw_imports,
      })
      .await
    }
    Command::Pack(args) => {
      jsr_cli::pack::pack(
        args.config.into_factory_options()?,
        PackFlags {
          files: FileFlags {
            include: args.files,
            ignore: args.ignore,
          },
          output: args.output,
          dry_run: args.dry_run,
          allow_slow_types: args.allow_slow_types,
          allow_dirty: args.allow_dirty,
          set_version: args.set_version,
          no_source_maps: args.no_source_maps,
        },
      )
      .await
    }
  }
}
