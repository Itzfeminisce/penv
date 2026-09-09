#!/usr/bin/env node
// Writes the launcher and the six platform packages a release publishes, from the
// raw binaries the build jobs produced:
//
//   node npm/pack.mjs --version v1.2.3 --binaries dist --out npm/build

import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = dirname(here);

// No libc field: the linux binary is statically linked against musl, so it runs on
// glibc hosts too, and naming one would keep npm from installing it on the other.
const TARGETS = [
  { platform: "linux", arch: "x64", triple: "x86_64-unknown-linux-musl" },
  { platform: "linux", arch: "arm64", triple: "aarch64-unknown-linux-musl" },
  { platform: "darwin", arch: "x64", triple: "x86_64-apple-darwin" },
  { platform: "darwin", arch: "arm64", triple: "aarch64-apple-darwin" },
  { platform: "win32", arch: "x64", triple: "x86_64-pc-windows-msvc" },
  { platform: "win32", arch: "arm64", triple: "aarch64-pc-windows-msvc" },
];

const flags = {};
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 2) {
  const name = argv[i].replace(/^--/, "");
  if (argv[i + 1] === undefined) throw new Error(`${argv[i]} needs a value.`);
  flags[name] = argv[i + 1];
}
if (!flags.version) throw new Error("pack.mjs needs --version, the release tag.");
if (!flags.binaries) throw new Error("pack.mjs needs --binaries, the directory the release built into.");

const version = flags.version.replace(/^v/, "");
const tag = `v${version}`;
const binaries = resolve(flags.binaries);
const out = resolve(flags.out ?? join(here, "build"));
rmSync(out, { recursive: true, force: true });

// The repository is Cargo.toml metadata, so the slug is written down once for the workspace.
const cargo = readFileSync(join(repo, "Cargo.toml"), "utf8");
const url = cargo
  .split(/^\[/m)
  .find((block) => block.startsWith("workspace.package]"))
  ?.match(/^repository\s*=\s*"([^"]+)"/m)?.[1];
if (!url) throw new Error("Cargo.toml [workspace.package] names no repository.");

const launcher = JSON.parse(readFileSync(join(here, "cli", "package.json"), "utf8"));
launcher.version = version;
launcher.repository = { type: "git", url: `git+${url}.git` };
launcher.optionalDependencies = Object.fromEntries(
  TARGETS.map(({ platform, arch }) => [`@penvhq/cli-${platform}-${arch}`, version]),
);

const write = (dir, manifest) => {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  copyFileSync(join(repo, "LICENSE"), join(dir, "LICENSE"));
  console.log(`${manifest.name}@${manifest.version} -> ${dir}`);
};

const launcherDir = join(out, "cli");
mkdirSync(join(launcherDir, "bin"), { recursive: true });
copyFileSync(join(here, "cli", "bin", "penv.js"), join(launcherDir, "bin", "penv.js"));
copyFileSync(join(here, "cli", "README.md"), join(launcherDir, "README.md"));
write(launcherDir, launcher);

for (const { platform, arch, triple } of TARGETS) {
  const exe = platform === "win32" ? ".exe" : "";
  const source = join(binaries, `penv-${tag}-${triple}${exe}`);
  if (!existsSync(source)) throw new Error(`${source} is not there, so ${triple} cannot be packed.`);

  const dir = join(out, `cli-${platform}-${arch}`);
  mkdirSync(join(dir, "bin"), { recursive: true });
  const binary = join(dir, "bin", `penv${exe}`);
  copyFileSync(source, binary);
  chmodSync(binary, 0o755);

  write(dir, {
    name: `@penvhq/cli-${platform}-${arch}`,
    version,
    description: `The ${platform} ${arch} binary for penv, the CLI of penv.cloud.`,
    license: launcher.license,
    homepage: launcher.homepage,
    repository: launcher.repository,
    os: [platform],
    cpu: [arch],
    // The launcher resolves the binary by this path, so the package publishes it rather
    // than leaving a deep reach into a package that names no entry points.
    exports: {
      [`./bin/penv${exe}`]: `./bin/penv${exe}`,
      "./package.json": "./package.json",
    },
    // Yarn PnP keeps a package zipped otherwise, and a zipped binary cannot be run.
    preferUnplugged: true,
  });
}
