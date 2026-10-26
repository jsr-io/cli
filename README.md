# jsr cli

The JSR command line tool for publishing packages, independent of Deno.

`src` contains the publish and pack logic, copied from
`cli/tools/publish` and `cli/tools/pack` in Deno 2.9, without the
dependencies on `deno_core` and `deno_runtime`. The `chore: copy ...` commits
contain the unmodified copies, so `git log -p` shows every change made to
them.

It's released as binaries on GitHub releases. The
[`jsr`](https://www.npmjs.com/package/jsr) npm package in `npm`, moved here from
[jsr-io/jsr-npm](https://github.com/jsr-io/jsr-npm), downloads and runs them
for `npx jsr publish`. `deno publish` will become a shim that does the same.

Unlike `deno publish`, it doesn't type check.

## Usage

```sh
jsr publish [--dry-run] [--token <TOKEN>] [--allow-slow-types] ...
jsr pack [--output <FILE>] [--dry-run] ...
```

The flags match `deno publish` and `deno pack`, and `jsr pack` produces the
same npm tarball as `deno pack`. `--check` and `--no-check` are accepted for
compatibility, but nothing is type checked. Set `JSR_LOG=debug` for verbose
output.

## Layout

- `cli`: the `jsr` binary (argument parsing and logging).
- `src`: the `jsr_cli` crate with the publish and pack logic.
- `npm`: the `jsr` npm package.

## Development

```sh
cargo test
cargo run -- publish --dry-run
```

`cargo test` also runs Deno's publish and pack tests (`tests/specs`,
`tests/integration`) against the `jsr` binary, with mock JSR and npm
registries (`tests/util/server`). A few fixtures use `deno` from the `PATH` to
set themselves up. To run a single spec test:

```sh
cargo test --test specs -- publish::dry_run
```

The npm package's tests need a `jsr` binary:

```sh
cargo build --bin jsr
cd npm && npm ci && JSR_BIN_PATH=$PWD/../target/debug/jsr node --run test
```

## Releasing

1. Set the same version in `cli/Cargo.toml` and `npm/package.json`.
2. Push a `v<version>` tag. The `release` workflow builds `jsr` for every
   target and creates a draft GitHub release with the zips and their
   `.sha256sum` files.
3. Publish the draft. The `npm publish` workflow writes the version and the
   checksums into `npm/src/cli_version.ts` and publishes the npm package.
