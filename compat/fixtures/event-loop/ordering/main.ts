import assert from "node:assert/strict";

const events: string[] = ["sync"];
queueMicrotask(() => events.push("queueMicrotask"));
Promise.resolve().then(() => events.push("promise"));
process.nextTick(() => events.push("nextTick"));

await Bun.sleep(1);
events.push("afterSleep");
await new Promise<void>(resolve =>
  setImmediate(() => {
    events.push("immediate");
    resolve();
  }),
);

assert.deepEqual(events, [
  "sync",
  "nextTick",
  "queueMicrotask",
  "promise",
  "afterSleep",
  "immediate",
]);
console.log(JSON.stringify(events));
