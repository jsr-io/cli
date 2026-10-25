# jsr cli

The JSR command line tool for publishing packages, independent of Deno.

`crates/jsr_cli` contains the publish logic, copied from `cli/tools/publish`
in Deno 2.9, without the dependencies on `deno_core` and `deno_runtime`. It
will be published to JSR as `@jsr/cli`, and the
[`jsr`](https://www.npmjs.com/package/jsr) npm package becomes its npm entry
point. `deno publish` will run the JSR package, like `deno deploy` runs
`@deno/deploy`.

Unlike `deno publish`, it doesn't type check.

## Embedding

The embedder implements `jsr_cli::Host` for HTTP requests, sleeping, running
`git`, opening a browser and checking whether stdin is a terminal.

## Development

```sh
cargo test

# dry run publish for a directory, with a minimal native host
cargo run --example dry_run -- path/to/package
```
