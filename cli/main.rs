// Copyright 2018-2026 the Deno authors. MIT license.

mod logger;

use std::path::PathBuf;

use clap::Args;
use clap::Parser;
use clap::Subcommand;
use deno_terminal::colors;
use jsr_cli::FactoryOptions;
use jsr_cli::npm_project;
use jsr_cli::npm_project::DependencyKind;
use jsr_cli::npm_project::PkgManagerName;
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
  /// Add JSR packages to the package.json and install them
  #[command(visible_aliases = ["install", "i"])]
  Add(AddArgs),
  /// Remove JSR packages from the package.json
  #[command(visible_aliases = ["uninstall", "r"])]
  Remove(RemoveArgs),
  /// Show information about a JSR package
  #[command(visible_aliases = ["show", "view"])]
  Info(InfoArgs),
  /// Run a script from the package.json. `jsr <script>` works too
  Run(RunArgs),
  /// Publish the current working directory's package or workspace
  Publish(PublishArgs),
  /// Create a tarball of the package
  Pack(PackArgs),
  #[command(external_subcommand)]
  Script(Vec<String>),
}

/// Chooses the package manager. By default it's detected from the lockfile.
#[derive(Args)]
#[group(multiple = false)]
struct PackageManagerArgs {
  /// Use npm
  #[arg(long)]
  npm: bool,
  /// Use yarn
  #[arg(long)]
  yarn: bool,
  /// Use pnpm
  #[arg(long)]
  pnpm: bool,
  /// Use bun
  #[arg(long)]
  bun: bool,
}

impl PackageManagerArgs {
  fn name(&self) -> Option<PkgManagerName> {
    if self.npm {
      Some(PkgManagerName::Npm)
    } else if self.yarn {
      Some(PkgManagerName::Yarn)
    } else if self.pnpm {
      Some(PkgManagerName::Pnpm)
    } else if self.bun {
      Some(PkgManagerName::Bun)
    } else {
      None
    }
  }
}

#[derive(Args)]
struct AddArgs {
  /// Packages to add, like @std/encoding or @std/encoding@1. Without any,
  /// installs the existing dependencies
  packages: Vec<String>,

  /// Add to dependencies. This is the default
  #[arg(short = 'P', long, group = "dependency_kind")]
  save_prod: bool,

  /// Add to devDependencies
  #[arg(short = 'D', long, group = "dependency_kind")]
  save_dev: bool,

  /// Add to optionalDependencies
  #[arg(short = 'O', long, group = "dependency_kind")]
  save_optional: bool,

  #[command(flatten)]
  package_manager: PackageManagerArgs,
}

#[derive(Args)]
struct RemoveArgs {
  /// Packages to remove
  #[arg(required = true)]
  packages: Vec<String>,

  #[command(flatten)]
  package_manager: PackageManagerArgs,
}

#[derive(Args)]
struct InfoArgs {
  /// The package, like @std/encoding or @std/encoding@1
  package: String,
}

#[derive(Args)]
struct RunArgs {
  /// The script to run
  script: String,

  #[command(flatten)]
  package_manager: PackageManagerArgs,
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

  /// Don't warn about slow types. Modules with slow types get no .d.ts
  /// files either way
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
    if let Some(err) = err.downcast_ref::<npm_project::ExecError>() {
      log::error!("{}: {}", colors::red_bold("error"), err);
      std::process::exit(err.code);
    }
    log::error!(
      "{}: {}",
      colors::red_bold("error"),
      format!("{err:?}").trim_start_matches("error: ")
    );
    std::process::exit(1);
  }
}

async fn run(command: Command) -> Result<(), anyhow::Error> {
  let cwd = std::env::current_dir()?;
  match command {
    Command::Add(args) => {
      let kind = if args.save_dev {
        DependencyKind::Dev
      } else if args.save_optional {
        DependencyKind::Optional
      } else {
        DependencyKind::Prod
      };
      let name = args.package_manager.name();
      with_timing(npm_project::add(&cwd, &args.packages, kind, name)).await
    }
    Command::Remove(args) => {
      let name = args.package_manager.name();
      with_timing(async { npm_project::remove(&cwd, &args.packages, name) })
        .await
    }
    Command::Info(args) => npm_project::info(&args.package).await,
    Command::Run(args) => {
      let name = args.package_manager.name();
      with_timing(async { npm_project::run_script(&cwd, &args.script, name) })
        .await
    }
    Command::Script(args) => {
      let script = &args[0];
      if !npm_project::has_script(&cwd, script) {
        anyhow::bail!(
          "Unknown command: {script}. Run `jsr --help` for the commands."
        );
      }
      with_timing(async { npm_project::run_script(&cwd, script, None) }).await
    }
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

/// Runs a package manager command and logs how long it took.
async fn with_timing(
  future: impl Future<Output = Result<(), anyhow::Error>>,
) -> Result<(), anyhow::Error> {
  let start = std::time::Instant::now();
  future.await?;
  log::info!(
    "\n{} in {}",
    colors::green("Completed"),
    pretty_time(start.elapsed())
  );
  Ok(())
}

fn pretty_time(duration: std::time::Duration) -> String {
  let ms = duration.as_millis();
  if ms > 24 * 60 * 60 * 1000 {
    format!("{}d", ms / (24 * 60 * 60 * 1000))
  } else if ms > 60 * 60 * 1000 {
    format!("{}h", ms / (60 * 60 * 1000))
  } else if ms > 60 * 1000 {
    format!("{}m", ms / (60 * 1000))
  } else if ms > 1000 {
    format!("{}s", ms / 1000)
  } else {
    format!("{ms}ms")
  }
}
