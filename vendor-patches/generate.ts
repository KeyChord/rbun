#!/usr/bin/env bun
/**
 * Vendor Bun into `vendor/bun` and apply rbun's patches to it.
 *
 *   bun vendor-patches/generate.ts apply            re-apply codemods + files to vendor/bun (idempotent)
 *   bun vendor-patches/generate.ts check            verify vendor/bun matches what `apply` would produce
 *   bun vendor-patches/generate.ts upgrade <ref>    re-vendor Bun at <ref> (sha / tag / branch), then apply
 *   bun vendor-patches/generate.ts diff             regenerate bun.gen.patch against pristine upstream
 *
 * Each package under `codemods/<name>/` declares in `patch.json` which upstream
 * files its JSSG codemod edits (`bundles`) and which new files it adds verbatim
 * from its `files/` tree (`files`). `apply` copies the files, runs the codemod
 * over every bundle, and writes `bun.gen.patch` — a reviewable unified diff of
 * the patched files against pristine upstream (fetched from GitHub when no
 * checkout is at hand). The generator is dependency-free on purpose: Bun's
 * runtime plus `git`, `rsync`, and the `codemod` CLI from this package's
 * devDependencies are all it needs.
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const PATCHES_DIR = import.meta.dirname;
const ROOT = path.resolve(PATCHES_DIR, "..");
const CODEMODS_DIR = path.join(PATCHES_DIR, "codemods");
const GENERATED_PATCH = path.join(PATCHES_DIR, "bun.gen.patch");
const VENDORED_COMMIT_FILE = "VENDORED_COMMIT";
/** Top-level upstream directories not vendored (tests/benchmarks) and local build state to keep. */
const RSYNC_EXCLUDES = ["/.git", "/test", "/bench", "/build", "/node_modules", "/vendor", "/.cache"];

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
  /** Absolute path of the vendored tree this package patches. */
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
    packages.push({ codemodPath, config, dir, name: entry.name, targetDir: path.join(ROOT, config.target) });
  }
  if (packages.length === 0) {
    fail(`no patch packages found under ${CODEMODS_DIR}`);
  }
  return packages;
}

function run(cmd: string, args: string[], options: { cwd?: string; okExitCodes?: number[]; quiet?: boolean } = {}): string {
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

function ensureCodemodCli(): void {
  if (!fs.existsSync(path.join(PATCHES_DIR, "node_modules", ".bin", "codemod"))) {
    run("bun", ["install"], { cwd: PATCHES_DIR });
  }
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
  console.log(`\n=== ${pkg.name} → ${pkg.config.target} ===`);
  if (!fs.existsSync(pkg.targetDir)) {
    fail(`${pkg.config.target} does not exist; run \`upgrade <ref>\` first`);
  }
  copyFiles(pkg);
  for (const bundle of pkg.config.bundles) {
    const target = path.join(pkg.targetDir, bundle.path);
    if (!fs.existsSync(target)) {
      fail(`${pkg.name}: ${bundle.path} not present in ${pkg.config.target}`);
    }
    runJssg(pkg, target, bundle.language);
  }
}

/**
 * Verify the vendored tree already carries every patch: `files/` copies match
 * byte-for-byte and the codemod is a no-op on each bundle (run on a scratch
 * copy so `check` never mutates `vendor/bun`).
 */
function checkPackage(pkg: PatchPackage): boolean {
  console.log(`\n=== check ${pkg.name} → ${pkg.config.target} ===`);
  let ok = true;
  for (const file of pkg.config.files) {
    const expected = path.join(pkg.dir, "files", file);
    const actual = path.join(pkg.targetDir, file);
    if (!fs.existsSync(actual) || !fs.readFileSync(actual).equals(fs.readFileSync(expected))) {
      console.error(`MISMATCH ${file}: differs from ${path.relative(ROOT, expected)}`);
      ok = false;
    }
  }
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-patch-check-"));
  try {
    for (const bundle of pkg.config.bundles) {
      const actual = path.join(pkg.targetDir, bundle.path);
      if (!fs.existsSync(actual)) {
        console.error(`MISSING ${bundle.path}`);
        ok = false;
        continue;
      }
      const copy = path.join(scratch, bundle.path);
      fs.mkdirSync(path.dirname(copy), { recursive: true });
      fs.copyFileSync(actual, copy);
      runJssg(pkg, copy, bundle.language);
      if (!fs.readFileSync(copy).equals(fs.readFileSync(actual))) {
        console.error(`MISMATCH ${bundle.path}: codemod is not a no-op — run \`apply\``);
        ok = false;
      }
    }
  } finally {
    fs.rmSync(scratch, { force: true, recursive: true });
  }
  return ok;
}

function readVendoredCommit(pkg: PatchPackage): string {
  const file = path.join(pkg.targetDir, VENDORED_COMMIT_FILE);
  if (!fs.existsSync(file)) {
    fail(`${pkg.config.target}/${VENDORED_COMMIT_FILE} missing`);
  }
  return fs.readFileSync(file, "utf8").trim();
}

/** `https://github.com/<owner>/<repo>` → raw file URL at a commit. */
function rawUrl(repo: string, commit: string, file: string): string {
  const match = /^https:\/\/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?\/?$/.exec(repo);
  if (match === null) {
    fail(`cannot derive raw file URLs from repo ${repo} (expected a github.com URL)`);
  }
  return `https://raw.githubusercontent.com/${match[1]}/${match[2]}/${commit}/${file}`;
}

/** Materialise pristine copies of a package's bundles under `dir` (from a checkout or GitHub). */
async function materialisePristine(pkg: PatchPackage, dir: string, checkout: string | undefined): Promise<void> {
  const commit = readVendoredCommit(pkg);
  for (const bundle of pkg.config.bundles) {
    const destination = path.join(dir, bundle.path);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    if (checkout !== undefined) {
      fs.copyFileSync(path.join(checkout, bundle.path), destination);
      continue;
    }
    const url = rawUrl(pkg.config.repo, commit, bundle.path);
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`${response.status} fetching ${url}`);
    }
    fs.writeFileSync(destination, Buffer.from(await response.arrayBuffer()));
  }
}

/**
 * Write `bun.gen.patch`: pristine upstream vs the patched vendored tree for
 * every bundle and added file. `checkout` is a pristine upstream tree when one
 * is at hand (during `upgrade`); otherwise the files are fetched from GitHub.
 */
async function writeGeneratedPatch(packages: PatchPackage[], checkout: string | undefined): Promise<void> {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-patch-diff-"));
  try {
    const pristine = path.join(scratch, "a");
    const patched = path.join(scratch, "b");
    for (const pkg of packages) {
      await materialisePristine(pkg, pristine, checkout);
      for (const file of [...pkg.config.bundles.map((bundle) => bundle.path), ...pkg.config.files]) {
        const destination = path.join(patched, file);
        fs.mkdirSync(path.dirname(destination), { recursive: true });
        fs.copyFileSync(path.join(pkg.targetDir, file), destination);
      }
    }
    // `git diff --no-index` exits 1 when the trees differ, which is the point.
    const diff = run("git", ["diff", "--no-index", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", "a", "b"], {
      cwd: scratch,
      okExitCodes: [0, 1],
      quiet: true,
    });
    // Strip the scratch directory names (`a`/`b`) so the hunks read
    // `a/<path>` / `b/<path>`. For a file present on one side only git uses
    // that side's path for both headers, hence the `[ab]` after the prefix.
    const cleaned = diff.replaceAll(/^(diff --git |--- |\+\+\+ |rename (?:from|to) )(.*)$/gm, (line) =>
      line.replaceAll(/\b([ab])\/[ab]\//g, "$1/"),
    );
    const header =
      `# Generated by vendor-patches/generate.ts — do not edit by hand.\n` +
      `# Upstream: ${packages.map((pkg) => `${pkg.config.repo} @ ${readVendoredCommit(pkg)}`).join(", ")}\n` +
      `# Regenerate with: bun vendor-patches/generate.ts apply\n`;
    fs.writeFileSync(GENERATED_PATCH, header + cleaned);
    console.log(`wrote ${path.relative(ROOT, GENERATED_PATCH)}`);
  } finally {
    fs.rmSync(scratch, { force: true, recursive: true });
  }
}

async function writeGeneratedPatchBestEffort(packages: PatchPackage[], checkout: string | undefined): Promise<void> {
  try {
    await writeGeneratedPatch(packages, checkout);
  } catch (error) {
    console.warn(`warning: could not write ${path.relative(ROOT, GENERATED_PATCH)}: ${String(error)}`);
    console.warn("         (re-run `bun vendor-patches/generate.ts diff` with network access)");
  }
}

/** Shallow-clone `repo` at `ref` into a scratch directory; returns the resolved commit. */
function cloneUpstream(repo: string, ref: string, dir: string): string {
  run("git", ["init", "--quiet", dir]);
  run("git", ["-C", dir, "remote", "add", "origin", repo]);
  run("git", ["-C", dir, "fetch", "--depth=1", "origin", ref]);
  run("git", ["-C", dir, "checkout", "--quiet", "FETCH_HEAD"]);
  return run("git", ["-C", dir, "rev-parse", "HEAD"], { quiet: true }).trim();
}

/** Replace the vendored tree with the checkout, keeping local build state (see RSYNC_EXCLUDES). */
function syncVendoredTree(checkout: string, targetDir: string): void {
  fs.mkdirSync(targetDir, { recursive: true });
  run("rsync", ["-a", "--delete", ...RSYNC_EXCLUDES.map((exclude) => `--exclude=${exclude}`), `${checkout}/`, `${targetDir}/`]);
}

async function upgrade(packages: PatchPackage[], ref: string): Promise<void> {
  const byTarget = new Map<string, PatchPackage[]>();
  for (const pkg of packages) {
    byTarget.set(pkg.targetDir, [...(byTarget.get(pkg.targetDir) ?? []), pkg]);
  }
  for (const [targetDir, group] of byTarget) {
    const repos = new Set(group.map((pkg) => pkg.config.repo));
    if (repos.size !== 1) {
      fail(`packages targeting ${targetDir} disagree on the upstream repo: ${[...repos].join(", ")}`);
    }
    const repo = group[0]!.config.repo;
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "rbun-upstream-"));
    try {
      console.log(`\n=== vendoring ${repo} @ ${ref} → ${path.relative(ROOT, targetDir)} ===`);
      const commit = cloneUpstream(repo, ref, scratch);
      syncVendoredTree(scratch, targetDir);
      fs.writeFileSync(path.join(targetDir, VENDORED_COMMIT_FILE), `${commit}\n`);
      console.log(`vendored ${commit}`);
      for (const pkg of group) {
        applyPackage(pkg);
      }
      await writeGeneratedPatchBestEffort(group, scratch);
    } finally {
      fs.rmSync(scratch, { force: true, recursive: true });
    }
  }
  console.log("\nUpgrade applied. Next: scripts/build-bun.sh, then cargo test.");
}

async function main(): Promise<void> {
  const [command, ...rest] = process.argv.slice(2);
  const packages = discoverPackages();
  switch (command) {
    case "apply": {
      ensureCodemodCli();
      for (const pkg of packages) {
        applyPackage(pkg);
      }
      await writeGeneratedPatchBestEffort(packages, undefined);
      return;
    }
    case "check": {
      ensureCodemodCli();
      let ok = true;
      for (const pkg of packages) {
        ok = checkPackage(pkg) && ok;
      }
      if (!ok) {
        fail("vendored tree is out of date with vendor-patches; run `bun vendor-patches/generate.ts apply`");
      }
      console.log("\nvendored tree matches vendor-patches");
      return;
    }
    case "upgrade": {
      const ref = rest[0];
      if (ref === undefined) {
        fail("usage: generate.ts upgrade <sha|tag|branch>");
      }
      ensureCodemodCli();
      await upgrade(packages, ref);
      return;
    }
    case "diff": {
      await writeGeneratedPatch(packages, undefined);
      return;
    }
    default:
      fail("usage: generate.ts <apply|check|upgrade <ref>|diff>");
  }
}

await main();
