#!/usr/bin/env bun
/**
 * Build the vendored Bun and link libbun_embed.dylib.
 *
 *   _build-bun            # release (the profile rbun expects)
 *   _build-bun --profile=debug-no-asan   # any bun build flag
 *
 * Requirements (macOS): brew install llvm@21 automake ccache cmake coreutils
 * gnu-sed go icu4c libiconv libtool ninja pkg-config ruby, rustup (the
 * pinned nightly in com/github/oven-sh/bun/dist/rust-toolchain.toml is installed on demand),
 * and a release `bun` on PATH.
 */

import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "../../../../../..");
const BUN_DIR = path.join(ROOT, "com/github/oven-sh/bun");
const SOURCE_DIR = path.join(BUN_DIR, "src");
const DIST_DIR = path.join(BUN_DIR, "dist");
const VENDOR_BIN = path.join(BUN_DIR, "_vendor.ts");

function fail(message: string): never {
  console.error(`error: ${message}`);
  process.exit(1);
}

function run(
  cmd: string,
  args: string[],
  options: { cwd?: string; env?: Record<string, string | undefined>; quiet?: boolean } = {},
): void {
  if (!options.quiet) {
    console.log(`$ ${cmd} ${args.join(" ")}`);
  }
  const result = Bun.spawnSync([cmd, ...args], {
    cwd: options.cwd ?? ROOT,
    env: { ...process.env, ...options.env },
    stderr: "inherit",
    stdout: "inherit",
  });
  if (result.exitCode !== 0) {
    fail(`${cmd} ${args.join(" ")} exited with ${result.exitCode}`);
  }
}

function gitSha(): string {
  const result = Bun.spawnSync(["git", "-C", SOURCE_DIR, "rev-parse", "HEAD"], {
    stderr: "inherit",
    stdout: "pipe",
  });
  if (result.exitCode !== 0) {
    fail(`git rev-parse HEAD in ${SOURCE_DIR} exited with ${result.exitCode}`);
  }
  return result.stdout.toString().trim();
}

function buildEnv(): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = {};
  // Bun bakes the enclosing repo's HEAD into the binary; report the source
  // submodule commit rather than rbun's outer repository commit.
  if (!process.env.GIT_SHA) {
    env.GIT_SHA = gitSha();
  }
  // Upstream pins `codegen-units = 1` for its shipped binary; we only consume the
  // dylib locally, so trade a little codegen quality for a much faster build.
  // (LTO is unaffected: bun's build already sets CARGO_PROFILE_RELEASE_LTO for
  // its cross-language ThinLTO link.) Env overrides the manifest, so the
  // vendored Cargo.toml stays pristine.
  if (!process.env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS) {
    env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16";
  }
  return env;
}

async function main(): Promise<void> {
  const buildArgs = process.argv.slice(2);
  const env = buildEnv();

  run(VENDOR_BIN, ["generate"]);

  if (!fs.existsSync(path.join(DIST_DIR, "node_modules"))) {
    run("bun", ["install"], { cwd: DIST_DIR, env });
  }

  run("bun", ["scripts/build.ts", "--profile=release", ...buildArgs], { cwd: DIST_DIR, env });
  run("bun", ["scripts/embed-dylib.ts"], { cwd: DIST_DIR, env });

  console.log(`built ${path.join(DIST_DIR, "build/release/libbun_embed.dylib")}`);
}

await main();
