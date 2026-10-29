// Copyright 2018-2026 the Deno authors. MIT license.

//! Module graph creation for publishing. Nothing is type checked.

use std::borrow::Cow;

use anyhow::Context;
use anyhow::Error as AnyError;
use anyhow::bail;
use deno_config::deno_json::ConfigFile;
use deno_config::deno_json::NodeModulesDirMode;
use deno_config::workspace::JsrPackageConfig;
use deno_graph::CheckJsOption;
use deno_graph::GraphKind;
use deno_graph::ModuleGraph;
use deno_graph::ModuleGraphError;
use deno_graph::WorkspaceFastCheckOption;
use deno_graph::ast::ParserModuleAnalyzer;
use deno_graph::source::Loader;
use deno_npm_installer::PackageCaching;
use deno_resolver::deno_json::JsxImportSourceConfigResolver;
use deno_resolver::graph::EnhanceGraphErrorMode;
use deno_resolver::graph::NpmTypesResolutionMode;
use deno_resolver::graph::enhance_graph_error;
use deno_resolver::graph::enhanced_integrity_error_message;
use deno_resolver::graph::format_deno_graph_error;
use deno_semver::StackString;
use url::Url;

use crate::Sys;
use crate::factory::PublishFactory;

pub type PublishGraphLoader = deno_resolver::file_fetcher::DenoGraphLoader<
  deno_cache_dir::file_fetcher::NullBlobStore,
  Sys,
  crate::file_fetcher::HttpClientAdapter,
>;

pub struct BuildFastCheckGraphOptions<'a> {
  /// Whether to do fast check on workspace members. This
  /// is mostly only useful when publishing.
  pub workspace_fast_check: WorkspaceFastCheckOption<'a>,
  /// Whether to generate .d.ts files.
  pub fast_check_dts: bool,
}

pub struct CreatePublishGraphOptions<'a> {
  pub packages: &'a [JsrPackageConfig],
  pub build_fast_check_graph: bool,
  pub validate_graph: bool,
}

#[derive(Debug, Clone)]
pub struct JsrUrlProvider(pub Url);

impl deno_graph::source::JsrUrlProvider for JsrUrlProvider {
  fn url(&self) -> &Url {
    &self.0
  }
}

pub struct ModuleGraphCreator<'a> {
  factory: &'a PublishFactory,
}

impl<'a> ModuleGraphCreator<'a> {
  pub fn new(factory: &'a PublishFactory) -> Self {
    Self { factory }
  }

  pub async fn create_publish_graph(
    &self,
    options: CreatePublishGraphOptions<'_>,
  ) -> Result<ModuleGraph, AnyError> {
    struct PublishLoader<'a> {
      loader: &'a PublishGraphLoader,
      jsr_url: &'a Url,
    }

    impl Loader for PublishLoader<'_> {
      fn load(
        &self,
        specifier: &deno_ast::ModuleSpecifier,
        options: deno_graph::source::LoadOptions,
      ) -> deno_graph::source::LoadFuture {
        if matches!(specifier.scheme(), "bun" | "virtual" | "cloudflare") {
          Box::pin(std::future::ready(Ok(Some(
            deno_graph::source::LoadResponse::External {
              specifier: specifier.clone(),
            },
          ))))
        } else if matches!(specifier.scheme(), "http" | "https")
          && !specifier.as_str().starts_with(self.jsr_url.as_str())
        {
          // mark non-JSR remote modules as external as these will error out
          // later when publishing
          Box::pin(std::future::ready(Ok(Some(
            deno_graph::source::LoadResponse::External {
              specifier: specifier.clone(),
            },
          ))))
        } else {
          self.loader.load(specifier, options)
        }
      }
    }

    let mut roots = Vec::new();
    for package_config in options.packages {
      roots.extend(package_config.config_file.resolve_export_value_urls()?);
    }

    let factory = self.factory;
    let resolver_factory = factory.resolver_factory();
    let loader = factory.graph_loader()?;
    let publish_loader = PublishLoader {
      loader: &loader,
      jsr_url: factory.jsr_url(),
    };
    let parsed_source_cache = resolver_factory.parsed_source_cache();
    let parser = parsed_source_cache.as_capturing_parser();
    let analyzer = ParserModuleAnalyzer::new(&parser);
    let jsx_import_source_config_resolver =
      JsxImportSourceConfigResolver::from_compiler_options_resolver(
        resolver_factory.compiler_options_resolver()?,
      )?;
    let cjs_tracker = resolver_factory.cjs_tracker()?;
    let deno_resolver = resolver_factory.deno_resolver().await?;
    let graph_resolver = deno_resolver.as_graph_resolver(
      cjs_tracker,
      &jsx_import_source_config_resolver,
      None,
      NpmTypesResolutionMode::FallbackToExecution,
    );
    let jsr_url_provider = JsrUrlProvider(factory.jsr_url().clone());
    let sys = factory.sys();

    let npm_installer_factory = factory.npm_installer_factory();
    npm_installer_factory
      .initialize_npm_resolution_if_managed()
      .await?;
    let npm_graph_resolver =
      npm_installer_factory.npm_deno_graph_resolver().await?;
    let maybe_lockfile = npm_installer_factory.maybe_lockfile().await?;

    // ensure an "npm install" is done if the user has explicitly
    // opted into using a node_modules directory
    if factory
      .workspace_directory()?
      .workspace
      .node_modules_dir()?
      == Some(NodeModulesDirMode::Auto)
      && let Some(npm_installer) =
        npm_installer_factory.npm_installer_if_managed().await?
    {
      let already_done = npm_installer
        .ensure_top_level_package_json_install()
        .await?;
      if !already_done {
        npm_installer.cache_packages(PackageCaching::All).await?;
      }
    }

    let mut graph = ModuleGraph::new(GraphKind::All);
    // populate the information from the lockfile
    if let Some(lockfile) = maybe_lockfile {
      lockfile.fill_graph(&mut graph);
    }
    let mut locker = maybe_lockfile.map(|l| l.as_deno_graph_locker());
    graph
      .build(
        roots,
        // do not include the tsconfig imports when publishing
        Vec::new(),
        &publish_loader,
        deno_graph::BuildOptions {
          file_system: sys,
          jsr_url_provider: &jsr_url_provider,
          jsr_version_resolver: Cow::Borrowed(
            resolver_factory.jsr_version_resolver()?.as_ref(),
          ),
          module_analyzer: &analyzer,
          npm_resolver: Some(npm_graph_resolver.as_ref()),
          locker: locker.as_mut().map(|l| l as _),
          resolver: Some(&graph_resolver),
          unstable_bytes_imports: factory.unstable_raw_imports(),
          unstable_css_imports: factory.unstable_raw_imports(),
          ..Default::default()
        },
      )
      .await;

    if options.validate_graph {
      self.graph_valid(&graph)?;
    }

    if options.build_fast_check_graph {
      let fast_check_workspace_members = options
        .packages
        .iter()
        .map(|p| config_to_deno_graph_workspace_member(&p.config_file))
        .collect::<Result<Vec<_>, _>>()?;
      self
        .build_fast_check_graph(
          &mut graph,
          BuildFastCheckGraphOptions {
            workspace_fast_check: WorkspaceFastCheckOption::Enabled(
              &fast_check_workspace_members,
            ),
            fast_check_dts: false,
          },
        )
        .await?;
    }

    Ok(graph)
  }

  pub async fn build_fast_check_graph(
    &self,
    graph: &mut ModuleGraph,
    options: BuildFastCheckGraphOptions<'_>,
  ) -> Result<(), AnyError> {
    if !graph.graph_kind().include_types() {
      return Ok(());
    }

    log::debug!("Building fast check graph");
    let resolver_factory = self.factory.resolver_factory();
    let parser = resolver_factory.parsed_source_cache().as_capturing_parser();
    let jsx_import_source_config_resolver =
      JsxImportSourceConfigResolver::from_compiler_options_resolver(
        resolver_factory.compiler_options_resolver()?,
      )?;
    let graph_resolver =
      resolver_factory.deno_resolver().await?.as_graph_resolver(
        resolver_factory.cjs_tracker()?,
        &jsx_import_source_config_resolver,
        None,
        NpmTypesResolutionMode::Strict,
      );
    let jsr_url_provider = JsrUrlProvider(self.factory.jsr_url().clone());
    graph.build_fast_check_type_graph(
      deno_graph::BuildFastCheckTypeGraphOptions {
        es_parser: Some(&parser),
        fast_check_cache: None,
        fast_check_dts: options.fast_check_dts,
        jsr_url_provider: &jsr_url_provider,
        resolver: Some(&graph_resolver),
        workspace_fast_check: options.workspace_fast_check,
      },
    );
    Ok(())
  }

  /// Check if the graph's roots and their deps are available. Returns
  /// `Err(_)` if there is a known module graph or resolution error
  /// statically reachable from the roots and not a dynamic import.
  fn graph_valid(&self, graph: &ModuleGraph) -> Result<(), AnyError> {
    let sys = self.factory.sys();
    let roots = graph.roots.iter().cloned().collect::<Vec<_>>();
    let compiler_options_resolver = self
      .factory
      .resolver_factory()
      .compiler_options_resolver()?;
    let workspace = &self.factory.workspace_directory()?.workspace;

    // surface integrity errors (ex. JSR checksum mismatches) first
    for error in graph.module_errors() {
      if let Some(message) = enhanced_integrity_error_message(error) {
        bail!("{}", message);
      }
    }

    let mut errors = graph
      .walk(
        roots.iter(),
        deno_graph::WalkOptions {
          check_js: CheckJsOption::Custom(compiler_options_resolver.as_ref()),
          // nothing is type checked, so only validate the code dependencies
          kind: GraphKind::CodeOnly,
          follow_dynamic: false,
          prefer_fast_check_graph: false,
        },
      )
      .errors();
    if let Some(error) = errors.next() {
      let is_root = match &error {
        ModuleGraphError::ResolutionError(_)
        | ModuleGraphError::TypesResolutionError(_) => false,
        ModuleGraphError::ModuleError(error) => {
          roots.contains(error.specifier())
        }
      };
      let bare_importable_pkg_names = workspace
        .resolver_jsr_pkgs()
        .map(|pkg| pkg.name)
        .collect::<Vec<_>>();
      let enhanced = enhance_graph_error(
        sys,
        error,
        if is_root {
          EnhanceGraphErrorMode::HideRange
        } else {
          EnhanceGraphErrorMode::ShowRange
        },
        &bare_importable_pkg_names,
      );
      bail!("{}", enhanced);
    }

    // finally surface the npm resolution result
    if let Err(err) = &graph.npm_dep_graph_result {
      bail!("{}", format_deno_graph_error(err));
    }
    Ok(())
  }
}

pub fn config_to_deno_graph_workspace_member(
  config: &ConfigFile,
) -> Result<deno_graph::WorkspaceMember, AnyError> {
  let name: StackString = match &config.json.name {
    Some(name) => name.as_str().into(),
    None => bail!("Missing 'name' field in config file."),
  };
  let version = match &config.json.version {
    Some(version) => {
      Some(deno_semver::Version::parse_standard(version).with_context(
        || format!("Invalid 'version' field in '{}'", config.specifier),
      )?)
    }
    None => None,
  };
  Ok(deno_graph::WorkspaceMember {
    base: config.specifier.join("./").unwrap(),
    name,
    version,
    exports: config.to_exports_config()?.into_map(),
  })
}
