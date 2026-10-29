// Copyright 2018-2026 the Deno authors. MIT license.

//! Publishing packages to JSR, packing them as npm tarballs, and managing
//! the JSR dependencies of npm projects.

mod factory;
mod file_fetcher;
mod git;
mod graph_util;
mod http_util;
mod no_slow_types;
mod npm;
pub mod npm_project;
pub mod pack;
pub mod publish;
mod registry;
mod unfurl_utils;
mod util;

pub use factory::FactoryOptions;

pub type Sys = sys_traits::impls::RealSys;

pub(crate) type NpmResolver<TSys = Sys> = deno_resolver::npm::NpmResolver<TSys>;
pub(crate) type NodeResolver<TSys = Sys> = node_resolver::NodeResolver<
  deno_resolver::npm::DenoInNpmPackageChecker,
  node_resolver::DenoIsBuiltInNodeModuleChecker,
  NpmResolver<TSys>,
  TSys,
>;
pub(crate) type NpmReqResolver<TSys = Sys> = deno_resolver::npm::NpmReqResolver<
  deno_resolver::npm::DenoInNpmPackageChecker,
  node_resolver::DenoIsBuiltInNodeModuleChecker,
  NpmResolver<TSys>,
  TSys,
>;
pub(crate) type PackageJsonResolver<TSys = Sys> =
  node_resolver::PackageJsonResolver<TSys>;
