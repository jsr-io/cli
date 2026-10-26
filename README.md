# jsr

Command line tools for [JSR](https://jsr.io), in two forms:

- The [`jsr`](https://www.npmjs.com/package/jsr) npm package, run with
  `npx jsr`. It manages JSR dependencies of npm, yarn, pnpm and bun projects,
  and publishes packages.
- The `jsr` binary, from the
  [GitHub releases](https://github.com/jsr-io/cli/releases). It publishes and
  packs packages. Builds exist for Linux (glibc and musl), macOS and Windows,
  each on x64 and arm64.

`npx jsr publish` downloads the binary of its own version and runs it.

## Commands

| Command                    | npm package | Binary | Description                                     |
| -------------------------- | ----------- | ------ | ----------------------------------------------- |
| `add`, `install`, `i`      | yes         |        | Add JSR packages to `package.json` and install. |
| `remove`, `uninstall`, `r` | yes         |        | Remove JSR packages.                            |
| `info`, `show`, `view`     | yes         |        | Show information about a JSR package.           |
| `run <script>`, `<script>` | yes         |        | Run a `package.json` script.                    |
| `publish`                  | yes         | yes    | Publish the package or workspace.               |
| `pack`                     |             | yes    | Create an npm tarball of the package.           |

```sh
# add a dependency, with the project's package manager
npx jsr add @std/encoding

# check everything without uploading, then publish
npx jsr publish --dry-run
npx jsr publish

# with the binary
jsr publish
jsr pack
```

`npx jsr --help`, `jsr publish --help` and `jsr pack --help` list the options.

### Publishing

`jsr publish` reads the package from `jsr.json(c)` or `deno.json(c)`. It opens
the browser to authenticate, unless you pass `--token`. On GitHub Actions it
authenticates with OIDC and adds a provenance attestation; pass
`--no-provenance` to skip the attestation.

**`jsr` doesn't type check.** Run `tsc` or `deno check` before publishing.

## Environment variables

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
