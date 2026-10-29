// Copyright 2018-2026 the Deno authors. MIT license.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error as AnyError;
use anyhow::bail;
use deno_config::deno_json::ConfigFile;
use deno_config::glob::FilePatterns;
use deno_config::glob::PathOrPattern;
use deno_config::glob::PathOrPatternSet;
use deno_config::workspace::JsrPackageConfig;
use deno_graph::ModuleGraph;
use deno_semver::jsr::JsrPackageReqReference;
use deno_semver::npm::NpmPackageReqReference;
use deno_semver::package::PackageReqReference;
use deno_terminal::colors;
use jsr_npm_tarball::DependencyKind;
use jsr_npm_tarball::NpmPackageJsonOptions;
use jsr_npm_tarball::NpmTarballOptions;

use crate::Sys as CliSys;
use crate::factory::FactoryOptions;
use crate::factory::PublishFactory;
use crate::publish::PrepareOptions;
use crate::publish::prepare_packages;
use crate::util::display::human_size;

/// The arguments of `deno pack`.
#[derive(Debug, Clone, Default)]
pub struct PackFlags {
  pub files: FileFlags,
  pub output: Option<String>,
  pub dry_run: bool,
  pub allow_slow_types: bool,
  pub allow_dirty: bool,
  pub set_version: Option<String>,
  pub no_source_maps: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FileFlags {
  pub ignore: Vec<String>,
  pub include: Vec<String>,
}

/// Creates the npm tarball of each package: the files that `publish` would
/// upload, converted the way the JSR npm registry converts them, under the
/// package's own name.
pub async fn pack(
  options: FactoryOptions,
  pack_flags: PackFlags,
) -> Result<(), AnyError> {
  let factory = PublishFactory::new(CliSys::default(), options)?;
  let initial_cwd = factory.options().cwd.clone();
  let start_dir = factory.workspace_directory()?;

  // Check if git repository is clean (unless --allow-dirty)
  if !pack_flags.allow_dirty
    && let Some(dirty) = crate::git::check_if_git_repo_dirty(&initial_cwd).await
  {
    bail!(
      "Git repository has uncommitted changes. Use --allow-dirty to pack anyway.\n{}",
      dirty
    );
  }

  // Get package configs
  let mut packages = start_dir.jsr_packages_for_publish();
  if packages.is_empty() {
    match start_dir.member_deno_json() {
      Some(deno_json) => {
        let Some(name) = deno_json.json.name.clone() else {
          bail!(
            "Missing 'name' field in '{}'. Add a package name like:\n  {{\n    \"name\": \"@scope/package-name\",\n    ...\n  }}",
            deno_json.specifier
          );
        };
        if deno_json.json.version.is_none() {
          bail!(
            "Missing 'version' field in '{}'. Add a version like:\n  {{\n    \"version\": \"1.0.0\",\n    ...\n  }}",
            deno_json.specifier
          );
        }
        if deno_json.json.exports.is_none() {
          bail!(
            "Missing 'exports' field in '{}'. Add an exports field like:\n  {{\n    \"exports\": \"./mod.ts\",\n    ...\n  }}",
            deno_json.specifier
          );
        }

        packages.push(JsrPackageConfig {
          name,
          member_dir: start_dir.workspace.root_dir().clone(),
          config_file: deno_json.clone(),
          license: deno_json
            .json
            .license
            .as_ref()
            .and_then(|l| l.as_str().map(|s| s.to_string())),
          should_publish: true,
        });
      }
      None => {
        bail!("No deno.json found in current directory");
      }
    }
  }

  for package in &mut packages {
    // Validate package name format
    if !package.name.starts_with('@') || !package.name.contains('/') {
      bail!(
        "Invalid package name '{}'. Package name must be in the format '@scope/name'",
        package.name
      );
    }

    if let Some(version) = &pack_flags.set_version {
      if deno_semver::Version::parse_standard(version).is_err() {
        bail!(
          "Invalid semver version '{}'. Please provide a valid semver version (e.g., 1.0.0)",
          version
        );
      }
      let mut config_file = package.config_file.as_ref().clone();
      config_file.json.version = Some(version.clone());
      package.config_file = deno_maybe_sync::new_rc(config_file);
    } else if package.config_file.json.version.is_none() {
      bail!(
        "Missing version in package '{}'. Add a version field or use --set-version",
        package.name
      );
    }
  }

  let config_files = packages
    .iter()
    .map(|package| (package.name.clone(), package.config_file.clone()))
    .collect::<Vec<_>>();
  let adjust_file_patterns =
    |package: &JsrPackageConfig, file_patterns: &mut FilePatterns| {
      adjust_file_patterns(package, file_patterns, &pack_flags, &initial_cwd)
    };
  let prepared = prepare_packages(
    &factory,
    packages,
    PrepareOptions {
      allow_slow_types: pack_flags.allow_slow_types,
      pack: true,
      adjust_file_patterns: Some(&adjust_file_patterns),
    },
  )
  .await?;

  for (name, config_file) in config_files {
    log::info!(
      "{} {}",
      colors::green("Packing"),
      colors::intense_blue(&name)
    );
    let package = &prepared.package_by_name[&name];
    let version = deno_semver::Version::parse_standard(&package.version)?;
    let files = package.tarball.read_files()?;
    let exports = config_file.to_exports_config()?.into_map();
    let graph =
      jsr_npm_tarball::build_package_graph(&name, &version, &exports, &files)
        .await
        .with_context(|| {
          format!("Failed to build module graph for package '{name}'")
        })?;
    let dependencies = collect_dependencies(&graph.graph);
    let transpile_options =
      jsr_npm_tarball::transpile_options_from_compiler_options(
        config_file.json.compiler_options.as_ref(),
      );
    let license = config_file
      .json
      .license
      .as_ref()
      .and_then(|l| l.as_str().map(|s| s.to_string()));

    let tarball = jsr_npm_tarball::create_npm_tarball(NpmTarballOptions {
      graph: &graph.graph,
      parser: &graph.analyzer.as_capturing_parser(),
      package_json: NpmPackageJsonOptions {
        name: &name,
        version: &version,
        homepage: None,
        license,
        jsr_revision: None,
      },
      exports: exports
        .iter()
        .map(|(key, path)| (key.as_str(), path.as_str())),
      files: &files,
      dependencies: dependencies.iter(),
      transpile_options: &transpile_options,
      source_maps: !pack_flags.no_source_maps,
    })
    .await
    .with_context(|| format!("Failed to create the tarball of '{name}'"))?;

    let tarball_path =
      output_path(&config_file, &package.version, &pack_flags)?;
    if pack_flags.dry_run {
      log::info!("Dry run - would create: {}", tarball_path.display());
      log::info!("\nPackage contents:");
      for entry in &tarball.entries {
        log::info!(
          "  {} ({})",
          entry.path.trim_start_matches('/'),
          human_size(entry.size as f64)
        );
      }
      log::info!("{} Dry run - no tarball created", colors::green("✓"));
    } else {
      std::fs::write(&tarball_path, &tarball.tarball).with_context(|| {
        format!("Failed writing '{}'", tarball_path.display())
      })?;
      log::info!(
        "{} {} ({})",
        colors::green("✓"),
        tarball_path.display(),
        human_size(tarball.tarball.len() as f64)
      );
    }
  }

  Ok(())
}

/// Applies the file patterns from the command line, and excludes the tarball
/// that `pack` writes.
fn adjust_file_patterns(
  package: &JsrPackageConfig,
  file_patterns: &mut FilePatterns,
  pack_flags: &PackFlags,
  cwd: &Path,
) -> Result<(), AnyError> {
  let package_dir = package.config_file.dir_path();
  // Unless the package lists what to publish, a `.tgz` at its root is likely
  // the output of an earlier `pack`, so it's left out.
  if file_patterns.include.is_none() {
    file_patterns
      .exclude
      .push(PathOrPattern::from_relative(&package_dir, "*.tgz")?);
  }
  if !pack_flags.files.include.is_empty() {
    file_patterns.include =
      Some(PathOrPatternSet::from_include_relative_path_or_patterns(
        &package_dir,
        &pack_flags.files.include,
      )?);
  }
  file_patterns.exclude.append(
    PathOrPatternSet::from_exclude_relative_path_or_patterns(
      &package_dir,
      &pack_flags.files.ignore,
    )?
    .into_path_or_patterns()
    .into_iter(),
  );
  let version = package.config_file.json.version.as_deref().unwrap_or("");
  let output = output_path(&package.config_file, version, pack_flags)?;
  file_patterns
    .exclude
    .push(PathOrPattern::Path(cwd.join(output)));
  Ok(())
}

fn output_path(
  config_file: &ConfigFile,
  version: &str,
  pack_flags: &PackFlags,
) -> Result<PathBuf, AnyError> {
  match &pack_flags.output {
    Some(output) => {
      let path = PathBuf::from(output);
      if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
      {
        log::warn!("Output path '{}' contains '..' components", output);
      }
      Ok(path)
    }
    None => default_tarball_filename(config_file, version),
  }
}

/// Compute the default tarball filename (`scope-name-version.tgz`) that
/// `pack` writes when no `--output` is given.
fn default_tarball_filename(
  config_file: &ConfigFile,
  version: &str,
) -> Result<PathBuf, AnyError> {
  let name = config_file
    .json
    .name
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
  // Convert @scope/name to scope-name
  let normalized = name.replace('@', "").replace('/', "-");
  // The package name shape is checked against `@scope/name` higher up, but
  // that check does not forbid path-traversal sequences. Reject any derived
  // tarball name that contains `..` or path separators so we never escape
  // the cwd.
  if normalized.contains("..") || normalized.contains('/') {
    return Err(anyhow::anyhow!(
      "refusing to write tarball with unsafe name derived from package: {}",
      name
    ));
  }
  Ok(PathBuf::from(format!("{}-{}.tgz", normalized, version)))
}

/// The jsr and npm packages that the package depends on.
fn collect_dependencies(
  graph: &ModuleGraph,
) -> Vec<(DependencyKind, PackageReqReference)> {
  let mut dependencies = Vec::new();
  for module in graph.modules() {
    let specifier = module.specifier().as_str();
    match module.specifier().scheme() {
      "jsr" => {
        if let Ok(req) = JsrPackageReqReference::from_str(specifier) {
          dependencies.push((DependencyKind::Jsr, req.into_inner()));
        }
      }
      "npm" => {
        if let Ok(req) = NpmPackageReqReference::from_str(specifier) {
          dependencies.push((DependencyKind::Npm, req.into_inner()));
        }
      }
      _ => {}
    }
  }
  dependencies
}
