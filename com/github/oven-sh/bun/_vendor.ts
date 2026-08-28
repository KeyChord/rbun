#!/usr/bin/env bun
/**
 * Materialize rbun's patched Bun distribution from the pinned Bun submodule.
 *
 *   _vendor generate
 *   _vendor check
 *   _vendor diff
 *   _vendor update <sha|tag|branch>
 *
 * `src/` is a pristine git submodule. `dist/` is ignored generated output.
 * Patch definitions and their generated review diff live under
 * `dev/improve/rbun/configs/patching`.
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const VENDOR_DIR = import.meta.dirname;
const ROOT = path.resolve(VENDOR_DIR, "../../../..");
const SOURCE_DIR = path.join(VENDOR_DIR, "src");
const DIST_DIR = path.join(VENDOR_DIR, "dist");
const PATCHES_DIR = path.join(ROOT, "dev/improve/rbun/configs/patching");
const CODEMODS_DIR = path.join(PATCHES_DIR, "codemods");
const GENERATED_PATCH = path.join(PATCHES_DIR, "bun.gen.patch");

// These are either unnecessary in the build distribution or expensive local
// build state. Excluded destination paths survive regeneration.
const DIST_EXCLUDES = ["/.git", "/test", "/bench", "/build", "/node_modules", "/vendor", "/.cache"];

interface Bundle {
  language: string;
  path: string;
}

interface PatchConfig {
  bundles: Bundle[];
  files: string[];
  repo: string;
  target: string;
}

interface PatchPackage {
  codemodPath: string;
  config: PatchConfig;
  dir: string;
  name: string;
  targetDir: string;
}

function fail(message: string): never {
  console.error(`error: ${message}`);
  process.exit(1);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parsePatchConfig(raw: string, configPath: string): PatchConfig {
  const parsed: unknown = JSON.parse(raw);
  if (
    !isRecord(parsed) ||
    typeof parsed.repo !== "string" ||
    typeof parsed.target !== "string" ||
    !Array.isArray(parsed.bundles) ||
    !Array.isArray(parsed.files)
  ) {
    fail(`invalid ${configPath}: expected { repo, target, bundles[], files[] }`);
  }
  const bundles = parsed.bundles.map((bundle: unknown): Bundle => {
    if (typeof bundle === "string") {
      return { language: "rust", path: bundle };
    }
    if (isRecord(bundle) && typeof bundle.path === "string" && typeof bundle.language === "string") {
      return { language: bundle.language, path: bundle.path };
    }
    return fail(`invalid bundle in ${configPath}: ${JSON.stringify(bundle)}`);
  });
  const files = parsed.files.map((file: unknown): string =>
    typeof file === "string" ? file : fail(`invalid file entry in ${configPath}: ${JSON.stringify(file)}`),
  );
  return { bundles, files, repo: parsed.repo, target: parsed.target };
}

function discoverPackages(): PatchPackage[] {
  const packages: PatchPackage[] = [];
  for (const entry of fs.readdirSync(CODEMODS_DIR, { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const dir = path.join(CODEMODS_DIR, entry.name);
    const configPath = path.join(dir, "patch.json");
    const codemodPath = path.join(dir, "codemod.ts");
    if (!fs.existsSync(configPath) || !fs.existsSync(codemodPath)) {
      continue;
    }
    const config = parsePatchConfig(fs.readFileSync(configPath, "utf8"), configPath);
    const targetDir = path.resolve(ROOT, config.target);
    if (targetDir !== DIST_DIR) {
      fail(`${configPath}: target must be ${path.relative(ROOT, DIST_DIR)}`);
    }
    packages.push({ codemodPath, config, dir, name: entry.name, targetDir });
  }
  if (packages.length === 0) {
    fail(`no patch packages found under ${CODEMODS_DIR}`);
  }
  return packages;
}

function run(
  cmd: string,
  args: string[],
  options: { cwd?: string; okExitCodes?: number[]; quiet?: boolean } = {},
): string {
  if (!options.quiet) {
    console.log(`$ ${cmd} ${args.join(" ")}`);
  }
  const result = Bun.spawnSync([cmd, ...args], {
    cwd: options.cwd ?? ROOT,
    stderr: "inherit",
    stdout: "pipe",
  });
  const stdout = result.stdout.toString();
  if (!options.quiet) {
    process.stdout.write(stdout);
  }
  if (!(options.okExitCodes ?? [0]).includes(result.exitCode)) {
    fail(`${cmd} ${args.join(" ")} exited with ${result.exitCode}`);
  }
  return stdout;
}

function ensureSourceInitialized(): void {
  if (!fs.existsSync(path.join(SOURCE_DIR, ".git"))) {
    run("git", ["submodule", "update", "--init", "--depth=1", "--", path.relative(ROOT, SOURCE_DIR)]);
  }
  if (!fs.existsSync(path.join(SOURCE_DIR, ".git"))) {
    fail(`Bun submodule is not initialized at ${path.relative(ROOT, SOURCE_DIR)}`);
  }
}

function sourceCommit(): string {
  ensureSourceInitialized();
  return run("git", ["-C", SOURCE_DIR, "rev-parse", "HEAD"], { quiet: true }).trim();
}

function requireCleanSource(): void {
  const changes = run("git", ["-C", SOURCE_DIR, "status", "--porcelain"], { quiet: true }).trim();
  if (changes.length > 0) {
    fail(`Bun source submodule has local changes:\n${changes}`);
  }
}

function ensureCodemodCli(): void {
  const installed =
    fs.existsSync(path.join(ROOT, "node_modules")) ||
    fs.existsSync(path.join(PATCHES_DIR, "node_modules"));
  if (!installed) {
    run("bun", ["install"], { cwd: ROOT });
  }
}

function rsyncArgs(): string[] {
  return DIST_EXCLUDES.map((exclude) => `--exclude=${exclude}`);
}

function syncTree(source: string, destination: string): void {
  fs.mkdirSync(destination, { recursive: true });
  run("rsync", ["-a", "--delete", ...rsyncArgs(), `${source}/`, `${destination}/`]);
}

/** Run a package's JSSG codemod over one file in place. */
function runJssg(pkg: PatchPackage, target: string, language: string): void {
  run(
    "bunx",
    [
      "codemod",
      "jssg",
      "run",
      "--target",
      target,
      "--language",
      language,
      "--allow-fs",
      "--allow-dirty",
      "--no-interactive",
      pkg.codemodPath,
    ],
    { cwd: PATCHES_DIR },
  );
}

function copyFiles(pkg: PatchPackage): void {
  for (const file of pkg.config.files) {
    const source = path.join(pkg.dir, "files", file);
    const destination = path.join(pkg.targetDir, file);
    if (!fs.existsSync(source)) {
      fail(`${pkg.name}: missing ${source} (declared in patch.json "files")`);
    }
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(source, destination);
    console.log(`copied ${file}`);
  }
}

function applyPackage(pkg: PatchPackage): void {
  console.log(`\n=== ${pkg.name} -> ${path.relative(ROOT, pkg.targetDir)} ===`);
  if (!fs.existsSync(pkg.targetDir)) {
    fail(`${path.relative(ROOT, pkg.targetDir)} does not exist; run generate first`);
  }
  copyFiles(pkg);
  for (const bundle of pkg.config.bundles) {
    const target = path.join(pkg.targetDir, bundle.path);
    if (!fs.existsSync(target)) {
      fail(`${pkg.name}: ${bundle.path} not present in ${path.relative(ROOT, pkg.targetDir)}`);
    }
    runJssg(pkg, target, bundle.language);
  }
}

function retarget(pkg: PatchPackage, targetDir: string): PatchPackage {
  return { ...pkg, targetDir };
}

function materializeDistribution(packages: PatchPackage[], destination = DIST_DIR): void {
  syncTree(SOURCE_DIR, destination);
  for (const pkg of packages) {
    applyPackage(retarget(pkg, destination));
  }
}

function generateDistribution(packages: PatchPackage[]): void {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-vendor-generate-"));
  try {
    const generated = path.join(scratch, "dist");
    materializeDistribution(packages, generated);
    fs.mkdirSync(DIST_DIR, { recursive: true });
    // Compare file content, not mtimes. This leaves an unchanged dist tree
    // untouched so repeated generation does not invalidate Ninja/Cargo state.
    run("rsync", ["-rclp", "--delete", ...rsyncArgs(), `${generated}/`, `${DIST_DIR}/`]);
  } finally {
    fs.rmSync(scratch, { force: true, recursive: true });
  }
}

function generatedPatchText(packages: PatchPackage[]): string {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-patch-diff-"));
  try {
    const pristine = path.join(scratch, "a");
    const patched = path.join(scratch, "b");
    for (const pkg of packages) {
      for (const bundle of pkg.config.bundles) {
        const source = path.join(SOURCE_DIR, bundle.path);
        const destination = path.join(pristine, bundle.path);
        fs.mkdirSync(path.dirname(destination), { recursive: true });
        fs.copyFileSync(source, destination);
      }
      for (const file of [...pkg.config.bundles.map((bundle) => bundle.path), ...pkg.config.files]) {
        const source = path.join(pkg.targetDir, file);
        const destination = path.join(patched, file);
        fs.mkdirSync(path.dirname(destination), { recursive: true });
        fs.copyFileSync(source, destination);
      }
    }
    const diff = run("git", ["diff", "--no-index", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", "a", "b"], {
      cwd: scratch,
      okExitCodes: [0, 1],
      quiet: true,
    });
    const cleaned = diff.replaceAll(/^(diff --git |--- |\+\+\+ |rename (?:from|to) )(.*)$/gm, (line) =>
      line.replaceAll(/\b([ab])\/[ab]\//g, "$1/"),
    );
    const repositories = [...new Set(packages.map((pkg) => pkg.config.repo))].join(", ");
    return (
      `# Generated by _vendor - do not edit by hand.\n` +
      `# Upstream: ${repositories} @ ${sourceCommit()}\n` +
      `# Regenerate with: _vendor generate\n` +
      cleaned
    );
  } finally {
    fs.rmSync(scratch, { force: true, recursive: true });
  }
}

function writeGeneratedPatch(packages: PatchPackage[]): void {
  fs.writeFileSync(GENERATED_PATCH, generatedPatchText(packages));
  console.log(`wrote ${path.relative(ROOT, GENERATED_PATCH)}`);
}

function checkDistribution(packages: PatchPackage[]): void {
  if (!fs.existsSync(DIST_DIR)) {
    fail(`${path.relative(ROOT, DIST_DIR)} does not exist; run generate`);
  }
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-vendor-check-"));
  try {
    const expected = path.join(scratch, "dist");
    materializeDistribution(packages, expected);
    const changes = run(
      "rsync",
      ["-nrcl", "--delete", "--out-format=%i %n", ...rsyncArgs(), `${expected}/`, `${DIST_DIR}/`],
      { quiet: true },
    )
      .split("\n")
      // This comparison is about generated content. Some rsync versions
      // report a no-transfer, timestamp-only item even without `--times`.
      .filter((line) => line.length > 0 && !line.startsWith("."))
      .join("\n");
    if (changes.length > 0) {
      fail(`generated Bun distribution is stale; run generate:\n${changes}`);
    }
    const expectedPatch = generatedPatchText(packages);
    const actualPatch = fs.existsSync(GENERATED_PATCH) ? fs.readFileSync(GENERATED_PATCH, "utf8") : "";
    if (actualPatch !== expectedPatch) {
      fail(`${path.relative(ROOT, GENERATED_PATCH)} is stale; run generate`);
    }
  } finally {
    fs.rmSync(scratch, { force: true, recursive: true });
  }
  console.log(`\n${path.relative(ROOT, DIST_DIR)} matches the pinned submodule plus patch configuration`);
}

function updateSource(ref: string): void {
  ensureSourceInitialized();
  requireCleanSource();
  run("git", ["-C", SOURCE_DIR, "fetch", "--depth=1", "origin", ref]);
  run("git", ["-C", SOURCE_DIR, "checkout", "--detach", "FETCH_HEAD"]);
  console.log(`pinned Bun source at ${sourceCommit()}`);
}

function printUsage(): void {
  console.log(`usage: _vendor <command>

commands:
  generate              recreate dist/ from src/ and apply every patch
  check                 verify dist/ and bun.gen.patch are reproducible
  diff                  rewrite bun.gen.patch from src/ versus dist/
  update <ref>          move the Bun submodule to ref, then generate
  test                  run the patch codemod fixture tests`);
}

async function main(): Promise<void> {
  const [command = "generate", ...rest] = process.argv.slice(2);
  const packages = discoverPackages();
  ensureSourceInitialized();
  requireCleanSource();

  switch (command) {
    case "generate":
    case "apply": {
      ensureCodemodCli();
      generateDistribution(packages);
      writeGeneratedPatch(packages);
      return;
    }
    case "check": {
      ensureCodemodCli();
      checkDistribution(packages);
      return;
    }
    case "diff": {
      writeGeneratedPatch(packages);
      return;
    }
    case "update":
    case "upgrade": {
      const ref = rest[0];
      if (ref === undefined) {
        fail(`usage: ${path.relative(ROOT, import.meta.path)} update <sha|tag|branch>`);
      }
      ensureCodemodCli();
      updateSource(ref);
      generateDistribution(packages);
      writeGeneratedPatch(packages);
      return;
    }
    case "test": {
      ensureCodemodCli();
      run(
        "bunx",
        [
          "codemod",
          "jssg",
          "test",
          "-l",
          "rust",
          path.join(PATCHES_DIR, "codemods/bun/codemod.ts"),
          path.join(PATCHES_DIR, "codemods/bun/@fixtures"),
        ],
        { cwd: PATCHES_DIR },
      );
      return;
    }
    case "help":
    case "--help":
    case "-h": {
      printUsage();
      return;
    }
    default:
      printUsage();
      fail(`unknown command: ${command}`);
  }
}

await main();
