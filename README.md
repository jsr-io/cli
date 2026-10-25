# jsr cli

The JSR command line tool for publishing packages, independent of Deno.

`crates/jsr_cli` contains the publish and pack logic, copied from
`cli/tools/publish` and `cli/tools/pack` in Deno 2.9, without the dependencies on `deno_core` and `deno_runtime`. The
first commit of this repository is the unmodified copy, so `git log -p` shows
every change made to it. It
will be published to JSR as `@jsr/cli`, and the
[`jsr`](https://www.npmjs.com/package/jsr) npm package becomes its npm entry
point. `deno publish` will run the JSR package, like `deno deploy` runs
`@deno/deploy`.

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

- `crates/jsr`: the `jsr` binary (argument parsing and logging).
- `crates/jsr_cli`: the publish and pack logic.

## Development

```sh
cargo test
cargo run -- publish --dry-run
```
