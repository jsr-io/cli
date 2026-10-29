# jsr

The command line tool for [JSR](https://jsr.io): it manages the JSR dependencies
of npm, yarn, pnpm and bun projects, and publishes and packs packages.

## Install

Run it from npm, without installing:

```sh
npx jsr <command>
```

The [`jsr`](https://www.npmjs.com/package/jsr) npm package downloads the `jsr`
binary of its version and runs it. You can also download the binary from the
[GitHub releases](https://github.com/jsr-io/cli/releases). It's built for Linux
(glibc and musl), macOS and Windows, each on x64 and arm64.

## Commands

| Command                    | Description                                       |
| -------------------------- | ------------------------------------------------- |
| `add`, `install`, `i`      | Add JSR packages to the package.json and install. |
| `remove`, `uninstall`, `r` | Remove JSR packages from the package.json.        |
| `info`, `show`, `view`     | Show information about a JSR package.             |
| `run <script>`, `<script>` | Run a script from the package.json.               |
| `publish`                  | Publish the package or workspace.                 |
| `pack`                     | Create an npm tarball of the package.             |

```sh
# add a dependency, with the project's package manager
jsr add @std/encoding

# check everything without uploading, then publish
jsr publish --dry-run
jsr publish

# create an npm tarball
jsr pack
```

`jsr --help` and `jsr <command> --help` list the options.

### Adding packages

`jsr add` detects the package manager from the lockfile, or use `--npm`,
`--yarn`, `--pnpm` or `--bun`. It points the `@jsr` scope at
`https://npm.jsr.io` in the `.npmrc` (yarn berry's config, or `bunfig.toml` for
bun before 1.1.18), and adds packages under their JSR name:

```json
{
  "dependencies": {
    "@std/encoding": "npm:@jsr/std__encoding@^1.0.0"
  }
}
```

### Publishing

`jsr publish` reads the package from `jsr.json(c)` or `deno.json(c)`. In a
project with a package.json and no deno.json, imports without file extensions
are resolved. It opens the browser to authenticate, unless you pass `--token`.
On GitHub Actions it authenticates with OIDC and adds a provenance attestation;
pass `--no-provenance` to skip the attestation.

**`jsr` doesn't type check.** Run `tsc` or `deno check` before publishing.

## Environment variables

| Variable            | Description                                                                    |
| ------------------- | ------------------------------------------------------------------------------ |
| `JSR_URL`           | Registry to publish to. Defaults to `https://jsr.io`.                          |
| `JSR_LOG`           | Log filter, for example `debug`.                                               |
| `JSR_LOG_STYLE`     | `auto`, `always` or `never` color in log output.                               |
| `DENO_DIR`          | Cache for downloaded dependencies.                                             |
| `DENO_CERT`         | Extra trusted CA certificates: a PEM file, or `base64:<pem>`.                  |
| `DENO_TLS_CA_STORE` | Trusted certificate stores, comma separated: `mozilla` (default) and `system`. |
| `JSR_BIN_PATH`      | npm package: run this `jsr` binary instead of downloading one.                 |

## Development

The repository contains:

- `cli`: the `jsr` binary, which parses arguments and sets up logging.
- `src`: the `jsr_cli` crate. `publish` and `pack` were copied from
  `cli/tools/publish` and `cli/tools/pack` in Deno 2.9, `npm_project` was ported
  from [jsr-io/jsr-npm](https://github.com/jsr-io/jsr-npm).
- `npm`: the [`jsr`](https://www.npmjs.com/package/jsr) npm package, which
  downloads and runs the binary.
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

The npm package's tests run `jsr` commands through it, with a local binary:

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
