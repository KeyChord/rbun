#!/usr/bin/env bun
/**
 * Initialize, classify, and differentially run Bun's upstream runtime tests
 * directly from rbun's pinned Bun source submodule.
 *
 *   _run-upstream-bun-tests sync
 *   _run-upstream-bun-tests classify
 *   _run-upstream-bun-tests run image
 *   _run-upstream-bun-tests run portable-runtime [substring]
 *
 * `RBUN_UPSTREAM_BUN_DIR` may point at another full/sparse checkout of the
 * same commit.
 */

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const SCRIPT = "_run-upstream-bun-tests";
const repo = resolve(import.meta.dirname, "../../../../../..");
const source = join(repo, "com/github/oven-sh/bun/src");
const config = JSON.parse(readFileSync(join(repo, "compat/upstream-suites.json"), "utf8"));
const checkout = resolve(process.env.RBUN_UPSTREAM_BUN_DIR ?? source);
const testRoot = join(checkout, "test");
const reference = resolve(
  process.env.RBUN_REFERENCE_BUN ?? join(repo, "com/github/oven-sh/bun/dist/build/release/bun"),
);
const candidate = resolve(process.env.RBUN_TEST_HOST ?? join(repo, "target/debug/rbun-test-host"));

function printUsage(): void {
  const suites = config.suites as Record<string, { description: string }>;
  console.log(`usage:
  bun ${SCRIPT} sync
  bun ${SCRIPT} classify
  bun ${SCRIPT} run <suite> [substring]

suites:
${Object.entries(suites)
  .map(([name, suite]) => `  ${name.padEnd(28)} ${suite.description}`)
  .join("\n")}`);
}

type Classification = "in-process" | "runtime-subprocess" | "cli" | "mixed";
type Observation = {
  status: number;
  stdout: string;
  stderr: string;
  timedOut: boolean;
};
type Summary = {
  pass: number;
  fail: number;
  skip: number;
  todo: number;
  expectations: number;
  files: number;
  unhandledErrors: number;
};

function runCommand(command: string[], cwd = repo, quiet = false): string {
  const result = Bun.spawnSync(command, { cwd, stdout: "pipe", stderr: "pipe" });
  const stdout = result.stdout.toString();
  const stderr = result.stderr.toString();
  if (!quiet) {
    process.stdout.write(stdout);
    process.stderr.write(stderr);
  }
  if (result.exitCode !== 0) {
    throw new Error(`${command.join(" ")} exited ${result.exitCode}`);
  }
  return stdout;
}

function ensureSourceSubmodule(): void {
  if (!existsSync(join(source, ".git"))) {
    runCommand(["git", "submodule", "update", "--init", "--depth=1", "--", "com/github/oven-sh/bun/src"]);
  }
  if (!existsSync(join(source, ".git"))) {
    throw new Error(`Bun source submodule is not initialized: ${source}`);
  }
}

function checkoutRevision(directory: string): string {
  return runCommand(["git", "-C", directory, "rev-parse", "HEAD"], repo, true).trim();
}

ensureSourceSubmodule();
const revision = checkoutRevision(source);

function syncCheckout(): void {
  if (checkout === source) {
    console.log(`Bun source submodule is initialized at ${revision}`);
    return;
  }
  if (!existsSync(join(checkout, ".git"))) {
    runCommand(["git", "init", checkout]);
    runCommand(["git", "-C", checkout, "remote", "add", "origin", "https://github.com/oven-sh/bun.git"]);
    runCommand(["git", "-C", checkout, "sparse-checkout", "init", "--cone"]);
    runCommand(["git", "-C", checkout, "sparse-checkout", "set", "test"]);
  }
  runCommand(["git", "-C", checkout, "fetch", "--depth=1", "origin", revision]);
  runCommand(["git", "-C", checkout, "checkout", "--detach", "FETCH_HEAD"]);
  const actual = checkoutRevision(checkout);
  if (actual !== revision) throw new Error(`upstream checkout is ${actual}, expected ${revision}`);
}

function requireCheckout(): void {
  if (!existsSync(testRoot)) {
    throw new Error(`upstream tests are missing; run: bun ${SCRIPT} sync`);
  }
  const actual = checkoutRevision(checkout);
  if (actual !== revision) {
    throw new Error(`upstream checkout is ${actual}, expected ${revision}; run sync`);
  }
}

function isTestFile(path: string): boolean {
  return /(?:\.test|\.spec)\.(?:[cm]?[jt]sx?)$/.test(path);
}

function discoverFiles(): string[] {
  const files: string[] = [];
  for (const root of config.sourceRoots as string[]) {
    const glob = new Bun.Glob("**/*");
    for (const path of glob.scanSync({ cwd: join(testRoot, root), onlyFiles: true })) {
      const absolute = join(testRoot, root, path);
      if (isTestFile(absolute)) files.push(`${root}/${path}`);
    }
  }
  return files.sort();
}

function classify(file: string): Classification {
  const source = readFileSync(join(testRoot, file), "utf8");
  const callsBunExe = /\bbunExe\s*\(/.test(source) || /\bbunRun\s*\(/.test(source);
  if (!callsBunExe) return "in-process";

  const commands = (config.cliSubcommands as string[]).map(escapeRegex).join("|");
  const invokesCli = new RegExp(
    `bunExe\\s*\\(\\s*\\)\\s*,\\s*["'](?:${commands})["']|bunRun\\s*\\(`,
  ).test(source);
  const invokesRuntime = /bunExe\s*\(\s*\)\s*,\s*["'](?:-e|--eval|--smol|--preload|[^"']+\.[cm]?[jt]sx?)["']/.test(
    source,
  );
  if (invokesCli && invokesRuntime) return "mixed";
  if (invokesCli) return "cli";
  return "runtime-subprocess";
}

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function selectSuite(name: string): string[] {
  const suite = config.suites[name];
  if (!suite) throw new Error(`unknown suite ${JSON.stringify(name)}`);
  if (suite.files) return [...suite.files];
  return discoverFiles().filter(file => classify(file) === suite.classification);
}

function printClassifications(): void {
  const files = discoverFiles();
  const counts = new Map<Classification, number>();
  for (const file of files) counts.set(classify(file), (counts.get(classify(file)) ?? 0) + 1);
  console.log(`revision ${revision}`);
  console.log(`test files ${files.length}`);
  for (const kind of ["in-process", "runtime-subprocess", "mixed", "cli"] as const) {
    console.log(`${kind.padEnd(20)} ${counts.get(kind) ?? 0}`);
  }
}

async function observe(command: string[], timeoutMs: number): Promise<Observation> {
  const proc = Bun.spawn(command, {
    cwd: testRoot,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0", RBUN_TEST_RESULT_JSON: "1" },
    stdout: "pipe",
    stderr: "pipe",
  });
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    proc.kill("SIGKILL");
  }, timeoutMs);
  const [status, stdout, stderr] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  clearTimeout(timer);
  return { status, stdout, stderr, timedOut };
}

function lastNumber(text: string, pattern: RegExp, fallback = 0): number {
  let value = fallback;
  for (const match of text.matchAll(pattern)) value = Number(match[1]);
  return value;
}

function referenceSummary(observation: Observation): Summary | null {
  const text = observation.stdout + "\n" + observation.stderr;
  if (!/Ran \d+ tests? across \d+ files?/.test(text)) return null;
  return {
    pass: lastNumber(text, /^\s*(\d+) pass\s*$/gm),
    fail: lastNumber(text, /^\s*(\d+) fail\s*$/gm),
    skip: lastNumber(text, /^\s*(\d+) skip\s*$/gm),
    todo: lastNumber(text, /^\s*(\d+) todo\s*$/gm),
    expectations: lastNumber(text, /(\d+) expect\(\) calls/g),
    files: lastNumber(text, /Ran \d+ tests? across (\d+) files?/g, 1),
    unhandledErrors: 0,
  };
}

function candidateSummary(observation: Observation): Summary | null {
  const marker = [...observation.stderr.matchAll(/^RBUN_TEST_RESULT (.+)$/gm)].at(-1);
  return marker ? JSON.parse(marker[1]) : null;
}

function sameSummary(a: Summary, b: Summary): boolean {
  return (Object.keys(a) as (keyof Summary)[]).every(key => a[key] === b[key]);
}

async function runSuite(name: string, filter?: string): Promise<void> {
  requireCheckout();
  if (!existsSync(reference)) throw new Error(`reference Bun missing: ${reference}`);
  runCommand(["cargo", "build", "--bin", "rbun-test-host"]);
  const revisionOutput = runCommand([reference, "--revision"], repo, true);
  if (!revisionOutput.includes(revision.slice(0, 9))) {
    throw new Error(`reference revision mismatch: ${revisionOutput.trim()}`);
  }

  let files = selectSuite(name);
  if (filter) files = files.filter(file => file.includes(filter));
  if (files.length === 0) throw new Error(`suite ${name} selected no files`);
  console.log(`suite ${name}: ${files.length} file(s), revision ${revision.slice(0, 9)}`);

  const timeoutMs = Number(process.env.RBUN_UPSTREAM_TIMEOUT_MS ?? 120_000);
  const failures: string[] = [];
  for (const file of files) {
    const referenceObservation = await observe([reference, "test", file], timeoutMs);
    const candidateObservation = await observe([candidate, "--rbun-test-file", file], timeoutMs);
    const expected = referenceSummary(referenceObservation);
    const actual = candidateSummary(candidateObservation);
    const okay =
      !referenceObservation.timedOut &&
      !candidateObservation.timedOut &&
      referenceObservation.status === 0 &&
      candidateObservation.status === 0 &&
      expected !== null &&
      actual !== null &&
      sameSummary(expected, actual);
    if (okay) {
      console.log(`PASS ${file} (${actual.pass} tests, ${actual.expectations} expects)`);
      continue;
    }
    failures.push(
      [
        file,
        `reference: status=${referenceObservation.status} timeout=${referenceObservation.timedOut} summary=${JSON.stringify(expected)}`,
        `candidate: status=${candidateObservation.status} timeout=${candidateObservation.timedOut} summary=${JSON.stringify(actual)}`,
        `candidate stderr:\n${candidateObservation.stderr.slice(-4000)}`,
      ].join("\n"),
    );
  }
  if (failures.length) {
    throw new Error(`${failures.length} upstream compatibility failure(s)\n\n${failures.join("\n\n")}`);
  }
}

const [command = "run", suite = "image", filter] = process.argv.slice(2);
if (command === "help" || command === "--help" || command === "-h") {
  printUsage();
} else if (command === "sync") {
  syncCheckout();
} else if (command === "classify") {
  requireCheckout();
  printClassifications();
} else if (command === "run") {
  await runSuite(suite, filter);
} else {
  throw new Error(`unknown command ${JSON.stringify(command)} (expected sync, classify, or run)`);
}
