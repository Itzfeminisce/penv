#!/usr/bin/env node
"use strict";

// npm installs one of the six @penvhq/cli-<platform> packages and skips the rest,
// so this shim runs whichever one landed. There is no postinstall step.
const { spawnSync } = require("node:child_process");

const pkg = `@penvhq/cli-${process.platform}-${process.arch}`;
const inside = `${pkg}/bin/penv${process.platform === "win32" ? ".exe" : ""}`;

let binary;
try {
  binary = require.resolve(inside);
} catch {
  process.stderr.write(
    `penv: ${pkg} is missing, and npm skips an optional dependency it could not install without saying so. ` +
      `Reinstall with npm i -g @penvhq/cli, or install without npm: curl -fsSL https://penv.cloud/install | sh, ` +
      `or on Windows irm https://penv.cloud/install.ps1 | iex\n`,
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  process.stderr.write(`penv: ${binary} could not be run: ${result.error.message}\n`);
  process.exit(1);
}
// Re-raised rather than translated, so a caller sees the signal that stopped penv.
if (result.signal) {
  process.kill(process.pid, result.signal);
  // Reached only when the signal was ignored, so there is still a code to answer with.
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
