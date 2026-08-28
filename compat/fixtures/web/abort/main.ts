import assert from "node:assert/strict";

const first = new AbortController();
const second = new AbortController();
const combined = AbortSignal.any([first.signal, second.signal]);
let events = 0;
combined.addEventListener("abort", () => events++, { once: true });
second.abort(new DOMException("compat stop", "AbortError"));
first.abort("ignored");
const result = {
  aborted: combined.aborted,
  events,
  reasonMessage: combined.reason.message,
  reasonName: combined.reason.name,
};
assert.deepEqual(result, {
  aborted: true,
  events: 1,
  reasonMessage: "compat stop",
  reasonName: "AbortError",
});
console.log(JSON.stringify(result));
