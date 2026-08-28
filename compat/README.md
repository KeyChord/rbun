# Bun differential compatibility suite

This suite runs the same fixture in two fresh processes and compares the
observable result:

1. `vendor/bun/build/release/bun` from `vendor/bun/VENDORED_COMMIT`, using
   `reference-driver.mjs` to dynamically import the fixture.
2. `rbun-compat-host`, which initializes rbun through its production
   `Runtime` / `Module::import` path, waits for module evaluation and event-loop
   work, and mirrors `process.exitCode`.

The comparison covers exit status, normalized stdout and stderr, and the full
case directory after execution (including files created or changed by the
fixture). Each case gets an independent temporary directory and VM process.
Only CRLF and the two different temporary-root paths are normalized.

## Run it

Build the vendored Bun and dylib first, then run:

```sh
scripts/build-bun.sh
cargo test --test bun_compat -- --nocapture
```

The test verifies that the reference executable's revision matches
`VENDORED_COMMIT`. These environment variables are available for focused or
external runs:

```sh
RBUN_COMPAT_FILTER=modules/ cargo test --test bun_compat -- --nocapture
RBUN_REFERENCE_BUN=/path/to/same-commit/bun cargo test --test bun_compat
RBUN_COMPAT_HOST=/path/to/rbun-compat-host cargo test --test bun_compat
```

`RBUN_COMPAT_FILTER` is a substring match against names such as
`modules/typescript`.

## Add a case

Create a directory under `fixtures/<area>/<case>/` containing a `case.json`:

```json
{
  "entry": "main.ts",
  "timeoutMs": 20000,
  "expectedStatus": 0
}
```

The timeout and expected status are optional (the defaults are 20 seconds and
zero). A reference timeout, signal, or unexpected exit status always fails,
so two equally broken runs cannot accidentally pass as compatible. Keep every
dependency and fixture inside that case directory. Tests should be ordinary
JS/TS modules and can use `node:assert/strict`; do not import `bun:test`,
because that module requires Bun's special test-runner VM rather than the
production runtime being compared here. Native `bun:test` coverage is handled
separately by `../scripts/bun-upstream-tests.ts`. Prefer deterministic output
and hermetic local resources.

When adapting an upstream Bun test, copy its runtime assertion and minimal
fixtures rather than its `bun:test` wrapper. Keep the upstream commit/path in a
comment when the case is substantially derived from one upstream test.

## Expected deviations

`expected-deviations.json` is an exact, reviewable allowlist. An entry names
the differing observation fields and snapshots the reference and candidate
values for those fields. The suite fails when:

- an unlisted difference appears;
- an expected value changes;
- the set of differing fields changes; or
- a listed difference disappears (XPASS), so the stale entry gets removed.

This provides a zero-*unexpected*-difference contract without hiding known
embedding semantics such as process identity and the continuing host's lack of
one-shot CLI `beforeExit` / `exit` shutdown events.

## Scope

The suite targets code executed inside the embedded Bun runtime: JavaScript,
TypeScript/JSX, ESM/CJS loading, package resolution, Node/Bun/Web APIs, event
loop behavior, I/O, exceptions, and observable process state. Bun CLI commands
(`bun install`, test discovery/CLI options, `bun build`, general argument
parsing, watch mode) are not part of rbun's embedding contract. The native
single-file test runner is covered by the pinned upstream suites below.
Rust-to-JavaScript conversion, callbacks, GC rooting, custom loaders, and host
futures remain covered by the Rust-facing tests in `../tests/` because the Bun
executable has no comparable API.

## Pinned upstream runtime suites

`../scripts/bun-upstream-tests.ts` is the complementary native-`bun:test`
harness. It sparse-checks out `test/` from the exact commit recorded in
`../vendor/bun/VENDORED_COMMIT`, then executes unchanged upstream files under
both that commit's Bun executable and rbun's embedded test VM. A file passes
only when both processes succeed and their pass/fail/skip/todo, assertion,
file, and unhandled-error counts match exactly.

```sh
bun scripts/bun-upstream-tests.ts sync
bun scripts/bun-upstream-tests.ts classify
bun scripts/bun-upstream-tests.ts run image
bun scripts/bun-upstream-tests.ts run webview-webkit
bun scripts/bun-upstream-tests.ts run runtime-smoke
bun scripts/bun-upstream-tests.ts run runtime-subprocess-smoke
```

The two broad suites are `portable-runtime` (no visible `bunExe()` call) and
`runtime-subprocess` (runtime-only child Bun calls). They can be narrowed with
a path substring as the final argument. Classification is deliberately a
reviewable file-level heuristic; `upstream-suites.json` records the named
files and explains why visible package-manager/bundler CLI cases and mixed
files are excluded.

Upstream's full test tree assumes various services, platform capabilities,
large fixtures, and development dependencies. Consequently the broad sweeps
are diagnostic rather than part of `cargo test`: a missing prerequisite is
reported as a reference failure and must not be counted as an rbun pass.
