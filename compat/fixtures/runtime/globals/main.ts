import assert from "node:assert/strict";

const result = {
  version: Bun.version,
  revision: Bun.revision,
  globals: {
    Bun: typeof Bun,
    Buffer: typeof Buffer,
    WebSocket: typeof WebSocket,
    fetch: typeof fetch,
    process: typeof process,
    queueMicrotask: typeof queueMicrotask,
    setTimeout: typeof setTimeout,
    structuredClone: typeof structuredClone,
  },
};

assert.match(result.version, /^\d+\.\d+\.\d+/);
assert.match(result.revision, /^[0-9a-f]{40}$/);
assert.deepEqual(result.globals, {
  Bun: "object",
  Buffer: "function",
  WebSocket: "function",
  fetch: "function",
  process: "object",
  queueMicrotask: "function",
  setTimeout: "function",
  structuredClone: "function",
});
console.log(JSON.stringify(result));
