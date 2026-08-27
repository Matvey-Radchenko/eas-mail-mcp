#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const packages = new Map([
  ["darwin-arm64", "eas-mail-mcp-darwin-arm64"],
  ["darwin-x64", "eas-mail-mcp-darwin-x64"],
  ["win32-x64", "eas-mail-mcp-win32-x64"],
]);
const platform = `${process.platform}-${process.arch}`;
const packageName = packages.get(platform);

if (!packageName) {
  fail(`Unsupported platform: ${platform}. Supported: macOS arm64/x64 and Windows x64.`);
}

const require = createRequire(import.meta.url);
let packageJson;
try {
  packageJson = require.resolve(`${packageName}/package.json`);
} catch {
  fail(
    `Native package ${packageName} is missing. Reinstall eas-mail-mcp with optional dependencies enabled.`,
  );
}

const binaryName = process.platform === "win32" ? "eas-mail-mcp.exe" : "eas-mail-mcp";
const binary = join(dirname(packageJson), "bin", binaryName);
const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: true,
});

const signals = process.platform === "win32" ? ["SIGINT", "SIGTERM"] : ["SIGINT", "SIGTERM", "SIGHUP"];
for (const signal of signals) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => fail(`Cannot start native eas-mail-mcp: ${error.message}`));
child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exitCode = code ?? 1;
  }
});

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}
