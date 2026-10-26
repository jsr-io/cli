const output = new Deno.Command(Deno.env.get("JSR_BIN")!, {
  args: ["publish"],
  clearEnv: true,
  env: {
    "GITHUB_ACTIONS": "true",
  },
  stdout: "inherit",
  stderr: "inherit",
}).outputSync();
Deno.exit(output.code);
