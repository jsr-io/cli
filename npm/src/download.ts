// Copyright the JSR authors. MIT license.

import * as crypto from "node:crypto";
import * as os from "node:os";
import * as fs from "node:fs";
import * as path from "node:path";
import * as util from "node:util";
import * as stream from "node:stream";
import StreamZipPkg from "node-stream-zip";
import { cliVersionInfo } from "./cli_version.ts";

const { async } = StreamZipPkg;

const streamFinished = util.promisify(stream.finished);

const styleText = typeof util.styleText === "function"
  ? util.styleText
  : (_style: string[] | string, text: string) => text;

// Example: https://github.com/jsr-io/cli/releases/download/v0.1.0/jsr-aarch64-apple-darwin.zip
const RELEASE_URL = "https://github.com/jsr-io/cli/releases/download";

const TARGETS: Record<string, string> = {
  "darwin arm64": "aarch64-apple-darwin",
  "darwin x64": "x86_64-apple-darwin",
  "linux arm64": "aarch64-unknown-linux-gnu",
  "linux x64": "x86_64-unknown-linux-gnu",
  "linux arm64 musl": "aarch64-unknown-linux-musl",
  "linux x64 musl": "x86_64-unknown-linux-musl",
  "win32 arm64": "aarch64-pc-windows-msvc",
  "win32 x64": "x86_64-pc-windows-msvc",
};

interface DownloadInfo {
  url: string;
  filename: string;
  version: string;
  target: string;
}

function getJsrDownloadUrl(): DownloadInfo {
  let key = `${process.platform} ${os.arch()}`;
  if (process.platform === "linux" && isMusl()) {
    key += " musl";
  }
  if (!(key in TARGETS)) {
    throw new Error(`Unsupported platform: ${key}`);
  }

  const target = TARGETS[key];
  const filename = `jsr-${target}.zip`;
  const version = cliVersionInfo.version;

  return {
    url: `${RELEASE_URL}/${version}/${filename}`,
    filename,
    version,
    target,
  };
}

function isMusl(): boolean {
  // Only glibc reports its version in the process report
  const report = process.report.getReport() as {
    header?: { glibcVersionRuntime?: string };
  };
  return report.header?.glibcVersionRuntime === undefined;
}

/**
 * Returns the path of the jsr binary of this package's version, downloading
 * it into `binFolder` first if needed.
 */
export async function getOrDownloadBinPath(binFolder: string) {
  const info = getJsrDownloadUrl();

  const binPath = path.join(
    binFolder,
    info.version,
    // Ensure each binary has their own folder to avoid overwriting it
    // in case jsr gets added to a project as a dependency where
    // developers use multiple OSes
    info.target,
    process.platform === "win32" ? "jsr.exe" : "jsr",
  );

  // Check if jsr executable is available, download it if not.
  if (!fs.existsSync(binPath)) {
    // Clear folder first to get rid of old download artifacts
    // to avoid taking up lots of disk space.
    await fs.promises.rm(binFolder, { recursive: true, force: true });

    await downloadJsr(binPath, info);
  }

  return binPath;
}

async function downloadJsr(
  binPath: string,
  info: DownloadInfo,
): Promise<void> {
  const expectedHash = cliVersionInfo.hashes[info.filename];
  if (expectedHash === undefined) {
    throw new Error(
      `No checksum for ${info.filename} in this jsr package. ` +
        `Set JSR_BIN_PATH to use a local jsr binary.`,
    );
  }

  const binFolder = path.dirname(binPath);

  await fs.promises.mkdir(binFolder, { recursive: true });

  const res = await fetch(info.url);
  if (!res.ok) {
    await res.body?.cancel();
    throw new Error(`${res.status}: Unable to download ${info.url}.`);
  }
  const contentLen = Number(res.headers.get("content-length") ?? Infinity);
  if (res.body == null) {
    throw new Error(`Unexpected empty body`);
  }

  console.log("Downloading JSR release binary...");

  await withProgressBar(
    async (tick) => {
      const tmpFile = path.join(binFolder, info.filename + ".part");
      const writable = fs.createWriteStream(tmpFile, "utf-8");
      const hash = crypto.createHash("sha256");

      for await (const chunk of streamToAsyncIterable(res.body!)) {
        tick(chunk.length);
        hash.update(chunk);
        writable.write(chunk);
      }

      writable.end();
      await streamFinished(writable);
      const file = path.join(binFolder, info.filename);
      await fs.promises.rename(tmpFile, file);

      const actualHash = hash.digest("hex");
      if (actualHash !== expectedHash) {
        await fs.promises.rm(file);
        throw new Error(
          `Hash mismatch for ${info.filename}:\n` +
            `  expected: ${expectedHash}\n` +
            `  actual:   ${actualHash}`,
        );
      }

      const zip = new async({ file });
      await zip.extract(null, binFolder);
      await zip.close();

      // Mark as executable
      await fs.promises.chmod(binPath, 493);

      // Delete downloaded file
      await fs.promises.rm(file);
    },
    { max: contentLen },
  );
}

async function withProgressBar<T>(
  fn: (tick: (n: number) => void) => Promise<T>,
  options: { max: number },
): Promise<T> {
  let current = 0;
  let start = Date.now();
  let passed = 0;
  let logged = false;

  const printStatus = throttle(() => {
    passed = Date.now() - start;

    const minutes = String(Math.floor(passed / 1000 / 60)).padStart(2, "0");
    const seconds = String(Math.floor(passed / 1000) % 60).padStart(2, "0");
    const time = `[${minutes}:${seconds}]`;
    const stats = `${humanFileSize(current)}/${humanFileSize(options.max)}`;

    const width = process.stdout.columns;

    let s = time;
    if (width - time.length - stats.length + 4 > 10) {
      const barLength = Math.min(width, 50);
      const percent = Math.floor((100 / options.max) * current);

      const bar = "#".repeat((barLength / 100) * percent) + ">";
      const remaining = styleText(
        "blue",
        "-".repeat(Math.max(barLength - bar.length, 0)),
      );
      s += ` [${styleText("cyan", bar)}${remaining}] `;
    }
    s += styleText("dim", stats);

    if (process.stdout.isTTY) {
      if (logged) {
        process.stdout.write("\r\x1b[K");
      }
      logged = true;
      process.stdout.write(s);
    }
  }, 16);

  const tick = (n: number) => {
    current += n;
    printStatus();
  };
  const res = await fn(tick);
  if (process.stdout.isTTY) {
    process.stdout.write("\n");
  } else {
    console.log("Download completed");
  }
  return res;
}

async function* streamToAsyncIterable<T>(
  stream: ReadableStream<T>,
): AsyncIterable<T> {
  const reader = stream.getReader();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) return;
      yield value;
    }
  } finally {
    reader.releaseLock();
  }
}

function humanFileSize(bytes: number, digits = 1): string {
  const thresh = 1024;

  if (Math.abs(bytes) < thresh) {
    return bytes + " B";
  }

  const units = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
  let u = -1;
  const r = 10 ** digits;

  do {
    bytes /= thresh;
    ++u;
  } while (
    Math.round(Math.abs(bytes) * r) / r >= thresh &&
    u < units.length - 1
  );

  return `${bytes.toFixed(digits)} ${units[u]}`;
}

function throttle(fn: () => void, delay: number): () => void {
  let timer: NodeJS.Timeout | null = null;

  return () => {
    if (timer === null) {
      fn();
      timer = setTimeout(() => {
        timer = null;
      }, delay);
    }
  };
}
