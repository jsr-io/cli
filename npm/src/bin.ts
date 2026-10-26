#!/usr/bin/env node
// Copyright the JSR authors. MIT license.

// Runs the jsr binary with the given arguments, downloading it first if
// needed.

import * as path from "node:path";
import { spawn } from "node:child_process";
import { getOrDownloadBinPath } from "./download.ts";

async function main() {
  const binPath = process.env.JSR_BIN_PATH ??
    await getOrDownloadBinPath(path.join(__dirname, "..", ".download"));

  const child = spawn(binPath, process.argv.slice(2), { stdio: "inherit" });
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.on(signal, () => child.kill(signal));
  }
  child.on("error", (err) => {
    console.error(err);
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    if (signal !== null) {
      // exit with the same signal
      process.removeAllListeners(signal);
      process.kill(process.pid, signal);
    } else {
      process.exit(code ?? 1);
    }
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
