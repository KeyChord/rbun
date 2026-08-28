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
dependency and fixture inside that case directory. Tests should be ordinary JS/TS modules and can use
`node:assert/strict`; do not import `bun:test`, because that module requires
Bun's special CLI test-runner VM rather than the production runtime being
compared here. Prefer deterministic output and hermetic local resources.

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
(`bun install`, `bun test`, `bun build`, argument parsing, watch mode) are not
part of rbun's embedding contract. Rust-to-JavaScript conversion, callbacks,
GC rooting, custom loaders, and host futures remain covered by the Rust-facing
tests in `../tests/` because the Bun executable has no comparable API.
