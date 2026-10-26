import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as module from "node:module";
import { isMainThread } from "node:worker_threads";
import { spawn } from "node:child_process";

export interface DenoJson {
  name: string;
  version: string;
  exports: string | Record<string, string>;
  license: string;
}

/**
 * By default when node run ts file it's in ESM context so __dirname is not defined.
 * But we use it in source code and `tsc` transpiles to ESM/CJS
 * So we need to deifnefpr test context.
 */
if (isMainThread && "register" in module) {
  const __dirname = import.meta.dirname;
  globalThis.__dirname = __dirname;
}

/**
 * This sets the `packageManager` field in the `package.json` of the
 * specified directory to be the latest modern stable version of yarn.
 */
export async function enableYarnBerry(cwd: string) {
  await exec("yarn", ["set", "version", "berry"], cwd);
}

export async function runJsr(
  args: string[],
  cwd: string,
  env: Record<string, string> = {},
  captureOutput = false,
) {
  const bin = path.resolve("src", "bin.ts");

  return await exec(
    "node",
    [
      "--no-warnings",
      "--import",
      import.meta.resolve("./test_utils.ts"),
      "--experimental-strip-types",
      bin,
      ...args,
    ],
    cwd,
    {
      ...process.env,
      npm_config_user_agent: undefined,
      ...env,
    },
    captureOutput,
  );
}

export async function runInTempDir(fn: (dir: string) => Promise<void>) {
  const dir = await fs.promises.mkdtemp(path.join(os.tmpdir(), "jsr-cli"));

  await writeJson(path.join(dir, "package.json"), {
    name: "jsr-test-package",
    version: "0.0.1",
    license: "MIT",
  });
  try {
    await fn(dir);
  } finally {
    await fs.promises.rm(dir, { recursive: true, force: true });
  }
}

export async function withTempEnv(
  args: string[],
  fn: (dir: string) => Promise<void>,
  options: { env?: Record<string, string> } = {},
): Promise<void> {
  await runInTempDir(async (dir) => {
    await runJsr(args, dir, options.env);
    await fn(dir);
  });
}

export async function isDirectory(path: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(path)).isDirectory();
  } catch (err) {
    return false;
  }
}

export async function isFile(path: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(path)).isFile();
  } catch (err) {
    return false;
  }
}

export interface PkgJson {
  name?: string;
  version?: string;
  license?: string;
  workspaces?: string[];

  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
  exports?: string | Record<string, string | Record<string, string>>;
  scripts?: Record<string, string>;
}

export class ExecError extends Error {
  public code: number;

  constructor(code: number) {
    super(`Child process exited with: ${code}`);
    this.code = code;
  }
}

export interface ExecOutput {
  combined: string;
  stdout: string;
  stderr: string;
}

export async function exec(
  cmd: string,
  args: string[],
  cwd: string,
  env?: Record<string, string | undefined>,
  captureOutput?: boolean,
): Promise<ExecOutput> {
  const cp = spawn(
    cmd,
    args.map((arg) => process.platform === "win32" ? `"${arg}"` : `'${arg}'`),
    {
      stdio: captureOutput ? "pipe" : "inherit",
      cwd,
      shell: true,
      env,
    },
  );

  let combined = "";
  let stdout = "";
  let stderr = "";

  if (captureOutput) {
    cp.stdout?.on("data", (data) => {
      combined += data;
      stdout += data;
    });
    cp.stderr?.on("data", (data) => {
      combined += data;
      stderr += data;
    });
  }

  return new Promise<ExecOutput>((resolve, reject) => {
    cp.on("exit", (code) => {
      if (code === 0) {
        resolve({ combined, stdout, stderr });
      } else {
        console.log(`Command: ${cmd} ${args.join(" ")}`);
        console.log(`CWD: ${cwd}`);
        console.log(`Output: ${combined}`);
        reject(new ExecError(code ?? 1));
      }
    });
  });
}

/** Bun reads .npmrc since v1.1.18, before that jsr writes a bunfig.toml. */
export async function isBunNpmrcSupported(cwd: string): Promise<boolean> {
  const output = await exec("bun", ["--version"], cwd, undefined, true);
  const [major, minor, patch] = output.stdout.trim().split(".").map(Number);
  return major > 1 ||
    (major === 1 && (minor > 1 || (minor === 1 && patch >= 18)));
}

export async function readJson<T>(file: string): Promise<T> {
  const content = await fs.promises.readFile(file, "utf-8");
  return JSON.parse(content);
}

export async function writeJson<T>(file: string, data: T): Promise<void> {
  await fs.promises.mkdir(path.dirname(file), { recursive: true });
  await fs.promises.writeFile(file, JSON.stringify(data, null, 2), "utf-8");
}

export async function readTextFile(file: string): Promise<string> {
  return fs.promises.readFile(file, "utf-8");
}

export async function writeTextFile(
  file: string,
  content: string,
): Promise<void> {
  await fs.promises.mkdir(path.dirname(file), { recursive: true });
  await fs.promises.writeFile(file, content, "utf-8");
}
