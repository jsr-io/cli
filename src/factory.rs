// Copyright 2018-2026 the Deno authors. MIT license.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Error as AnyError;
use deno_cache_dir::HttpCacheRc;
use deno_cache_dir::file_fetcher::CacheSetting;
use deno_cache_dir::file_fetcher::NullBlobStore;
use deno_config::workspace::WorkspaceDirectoryRc;
use deno_npm_cache::NpmCacheSetting;
use deno_npm_cache::NpmPackumentFormat;
use deno_npm_installer::NpmInstallerFactoryOptions;
use deno_npm_installer::graph::NpmCachingStrategy;
use deno_npm_installer::lifecycle_scripts::NullLifecycleScriptsExecutor;
use deno_resolver::factory::ConfigDiscoveryOption;
use deno_resolver::factory::ResolverFactory;
use deno_resolver::factory::ResolverFactoryOptions;
use deno_resolver::factory::WorkspaceFactory;
use deno_resolver::factory::WorkspaceFactoryOptions;
use deno_resolver::file_fetcher::DenoGraphLoaderOptions;
use deno_resolver::file_fetcher::PermissionedFileFetcher;
use deno_resolver::file_fetcher::PermissionedFileFetcherOptions;
use deno_resolver::workspace::PackageJsonDepResolution;
use deno_terminal::colors;
use node_resolver::NodeConditionOptions;
use node_resolver::NodeResolverOptions;
use url::Url;

use crate::Sys;
use crate::file_fetcher::HttpClientAdapter;
use crate::graph_util::PublishGraphLoader;
use crate::http_util::HttpClientProvider;
use crate::npm::NpmCacheHttpClient;
use crate::npm::NpmInstallerFactory;

/// The TypeScript version used when resolving `typesVersions` in npm
/// packages.
const TYPESCRIPT_VERSION: &str = "6.0.3";

#[derive(Debug, Clone, Default)]
pub struct FactoryOptions {
  pub cwd: PathBuf,
  /// Path to a config file, relative to `cwd`. When `None`, the config file
  /// is discovered from `cwd`.
  pub config: Option<PathBuf>,
  /// Disables automatic loading of the config file.
  pub no_config: bool,
  pub sloppy_imports: bool,
  pub unstable_raw_imports: bool,
}

/// Creates and holds the services needed for publishing.
pub struct PublishFactory {
  sys: Sys,
  http_client_provider: Arc<HttpClientProvider>,
  options: FactoryOptions,
  resolver_factory: Arc<ResolverFactory<Sys>>,
  npm_installer_factory: NpmInstallerFactory,
  jsr_url: Url,
  unstable_raw_imports: bool,
}

impl PublishFactory {
  pub fn new(sys: Sys, options: FactoryOptions) -> Result<Self, AnyError> {
    let http_client_provider = Arc::new(HttpClientProvider::default());
    let workspace_factory = Arc::new(WorkspaceFactory::new(
      sys.clone(),
      options.cwd.clone(),
      WorkspaceFactoryOptions {
        additional_config_file_names: &["jsr.json", "jsr.jsonc"],
        config_discovery: match &options.config {
          Some(path) => ConfigDiscoveryOption::Path(options.cwd.join(path)),
          None if options.no_config => ConfigDiscoveryOption::Disabled,
          None => ConfigDiscoveryOption::DiscoverCwd,
        },
        ..Default::default()
      },
    ));
    let workspace = &workspace_factory.workspace_directory()?.workspace;
    for diagnostic in workspace.diagnostics() {
      log::warn!("{} {}", colors::yellow("Warning"), diagnostic);
    }
    let unstable_sloppy_imports = options.sloppy_imports
      || workspace.has_unstable("sloppy-imports")
      || is_npm_project(workspace);
    let unstable_raw_imports =
      options.unstable_raw_imports || workspace.has_unstable("raw-imports");
    let jsr_url = deno_resolver::factory::resolve_jsr_url(&sys);
    let resolver_factory = Arc::new(ResolverFactory::new(
      workspace_factory,
      ResolverFactoryOptions {
        node_resolver_options: NodeResolverOptions {
          conditions: NodeConditionOptions {
            conditions: Vec::<Cow<'static, str>>::new(),
            import_conditions_override: None,
            require_conditions_override: None,
          },
          typescript_version: Some(
            deno_semver::Version::parse_standard(TYPESCRIPT_VERSION).unwrap(),
          ),
          bundle_mode: false,
          is_browser_platform: false,
        },
        unstable_sloppy_imports,
        on_mapped_resolution_diagnostic: Some(Arc::new(on_resolve_diagnostic)),
        // the node_modules directory is not published to jsr, so resolve
        // dependencies via the package.json rather than using node resolution
        package_json_dep_resolution: Some(PackageJsonDepResolution::Enabled),
        ..Default::default()
      },
    ));
    let npm_installer_factory =
      create_npm_installer_factory(&resolver_factory, &http_client_provider);
    Ok(Self {
      sys,
      http_client_provider,
      options,
      resolver_factory,
      npm_installer_factory,
      jsr_url,
      unstable_raw_imports,
    })
  }

  pub fn sys(&self) -> &Sys {
    &self.sys
  }

  pub fn http_client_provider(&self) -> &Arc<HttpClientProvider> {
    &self.http_client_provider
  }

  pub fn npm_installer_factory(&self) -> &NpmInstallerFactory {
    &self.npm_installer_factory
  }

  pub fn options(&self) -> &FactoryOptions {
    &self.options
  }

  pub fn resolver_factory(&self) -> &ResolverFactory<Sys> {
    &self.resolver_factory
  }

  pub fn workspace_directory(&self) -> Result<&WorkspaceDirectoryRc, AnyError> {
    Ok(
      self
        .resolver_factory
        .workspace_factory()
        .workspace_directory()?,
    )
  }

  pub fn jsr_url(&self) -> &Url {
    &self.jsr_url
  }

  pub fn jsr_api_url(&self) -> Url {
    let mut jsr_api_url = self.jsr_url.clone();
    jsr_api_url.set_path("api/");
    jsr_api_url
  }

  pub fn unstable_raw_imports(&self) -> bool {
    self.unstable_raw_imports
  }

  pub fn graph_loader(&self) -> Result<PublishGraphLoader, AnyError> {
    let workspace_factory = self.resolver_factory.workspace_factory();
    let http_cache: HttpCacheRc =
      Arc::new(workspace_factory.http_cache()?.clone());
    let memory_files = Arc::new(deno_resolver::loader::MemoryFiles::default());
    let file_fetcher = Arc::new(PermissionedFileFetcher::new(
      NullBlobStore,
      http_cache,
      HttpClientAdapter {
        http_client_provider: self.http_client_provider.clone(),
      },
      memory_files,
      self.sys.clone(),
      PermissionedFileFetcherOptions {
        allow_remote: true,
        cache_setting: CacheSetting::Use,
      },
    ));
    Ok(PublishGraphLoader::new(
      file_fetcher,
      workspace_factory.global_http_cache()?.clone(),
      self.resolver_factory.in_npm_package_checker()?.clone(),
      self.sys.clone(),
      DenoGraphLoaderOptions {
        file_header_overrides: Default::default(),
        include_npm_sources: false,
        permissions: None,
        file_permission_api_name: None,
        reporter: None,
      },
    ))
  }
}

fn create_npm_installer_factory(
  resolver_factory: &Arc<ResolverFactory<Sys>>,
  http_client_provider: &Arc<HttpClientProvider>,
) -> NpmInstallerFactory {
  // the `no-downgrade` trust policy reads `_npmUser`/`attestations`, which
  // are only present in the full packument
  let needs_full_packument_for_trust = resolver_factory
    .workspace_factory()
    .npmrc()
    .ok()
    .map(|rc| rc.trust_policy != deno_npmrc::TrustPolicyConfig::Off)
    .unwrap_or(false);
  let needs_full_packument = needs_full_packument_for_trust
    || resolver_factory
      .minimum_dependency_age_config()
      .ok()
      .and_then(|c| c.age.as_ref().and_then(|d| d.into_option()))
      .is_some();
  NpmInstallerFactory::new(
    resolver_factory.clone(),
    Arc::new(NpmCacheHttpClient::new(
      http_client_provider.clone(),
      if needs_full_packument {
        NpmPackumentFormat::Full
      } else {
        NpmPackumentFormat::Abbreviated
      },
    )),
    // lifecycle scripts are never run when publishing
    Arc::new(NullLifecycleScriptsExecutor),
    deno_npm_installer::LogReporter,
    None,
    NpmInstallerFactoryOptions {
      cache_setting: NpmCacheSetting::Use,
      caching_strategy: NpmCachingStrategy::Eager,
      clean_on_install: false,
      dedup_lockfile_peer_variants: true,
      lifecycle_scripts_config: Default::default(),
      production: false,
      skip_types: false,
      resolve_npm_resolution_snapshot: Box::new(|| Ok(None)),
    },
  )
}

fn on_resolve_diagnostic(
  diagnostic: deno_resolver::graph::MappedResolutionDiagnosticWithPosition,
) {
  log::warn!(
    "{} {}\n    at {}:{}",
    colors::yellow("Warning"),
    diagnostic.diagnostic,
    diagnostic.referrer,
    diagnostic.start
  );
}

/// Whether the workspace is an npm project, with a package.json and no
/// deno.json. Extensionless imports are common in npm projects, so they get
/// sloppy imports.
fn is_npm_project(workspace: &deno_config::workspace::Workspace) -> bool {
  workspace.package_jsons().next().is_some()
    && workspace.deno_jsons().all(|config| {
      let file_name = config.specifier.path().rsplit('/').next().unwrap_or("");
      file_name.starts_with("jsr.json")
    })
}
