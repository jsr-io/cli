// Copyright 2018-2026 the Deno authors. MIT license.

//! Publishing packages to JSR.
//!
//! The publish logic was copied from `cli/tools/publish` in the Deno
//! repository (Deno 2.9). It doesn't depend on `deno_core` or
//! `deno_runtime`, so it builds for `wasm32-unknown-unknown`.

mod factory;
mod file_fetcher;
mod git;
mod graph_util;
mod http_util;
mod no_slow_types;
mod npm;
pub mod publish;
mod registry;
mod unfurl_utils;
mod util;

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
