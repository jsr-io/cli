# jsr

The command line tool for publishing packages to [JSR](https://jsr.io) and
packing them as npm tarballs.

## Install

From npm, which also adds commands for installing JSR packages into npm, yarn,
pnpm and bun projects:

```sh
npx jsr publish
```

Or download a binary from the
[GitHub releases](https://github.com/jsr-io/cli/releases). They are built for
Linux (glibc and musl), macOS and Windows, each on x64 and arm64.

## Usage

```sh
# check everything, without uploading
jsr publish --dry-run

# publish the package or workspace in the current directory
jsr publish

# create an npm tarball of the package
jsr pack
```

`jsr publish` reads the package from `jsr.json(c)` or `deno.json(c)`. It opens
the browser to authenticate, unless you pass `--token`. On GitHub Actions it
authenticates with OIDC and adds a provenance attestation; pass
`--no-provenance` to skip the attestation.

Run `jsr publish --help` or `jsr pack --help` to list all options.

**`jsr` doesn't type check.** Run `tsc` or `deno check` before publishing.

### Environment variables

| Variable        | Description                                                         |
| --------------- | ------------------------------------------------------------------- |
| `JSR_URL`       | Registry to publish to. Defaults to `https://jsr.io`.               |
| `JSR_LOG`       | Log filter, for example `debug`.                                    |
| `JSR_LOG_STYLE` | `auto`, `always` or `never` color in log output.                    |
| `DENO_DIR`      | Cache for downloaded dependencies.                                  |
| `JSR_BIN_PATH`  | npm package only: run this `jsr` binary instead of downloading one. |

## Development

The repository contains:

- `cli`: the `jsr` binary, which parses arguments and sets up logging.
- `src`: the `jsr_cli` crate with the publish and pack logic, copied from
  `cli/tools/publish` and `cli/tools/pack` in Deno 2.9.
- `npm`: the [`jsr`](https://www.npmjs.com/package/jsr) npm package, moved here
  from [jsr-io/jsr-npm](https://github.com/jsr-io/jsr-npm).
- `tests`: Deno's publish and pack tests, and mock JSR and npm registries.

Code copied from Deno or jsr-npm lands in a `chore: copy ...` commit unmodified,
so `git log -p` shows every change made to it.

```sh
cargo test
cargo run -- publish --dry-run
```

`cargo test` runs Deno's publish and pack tests against the `jsr` binary. A few
of them use `deno` from the `PATH` to set themselves up. To run one:

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
2. Push a `v<version>` tag. The `release` workflow builds `jsr` for every target
   and creates a draft GitHub release with the zips and their `.sha256sum`
   files.
3. Publish the draft. The `npm publish` workflow writes the version and the
   checksums into `npm/src/cli_version.ts` and publishes the npm package.
