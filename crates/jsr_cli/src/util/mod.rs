// Copyright 2018-2026 the Deno authors. MIT license.

pub mod console;
pub mod display;
pub mod retry;

pub fn is_builtin_node_module(module_name: &str) -> bool {
  use node_resolver::IsBuiltInNodeModuleChecker;
  node_resolver::DenoIsBuiltInNodeModuleChecker
    .is_builtin_node_module(module_name)
}
