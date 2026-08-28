import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";

const events: string[] = [];
const cancelled = setTimeout(() => events.push("cancelled-fired"), 5);
clearTimeout(cancelled);
await sleep(10, "timer-value").then(value => events.push(value));

await new Promise<void>(resolve => {
  let ticks = 0;
  const interval = setInterval(() => {
    events.push(`interval-${++ticks}`);
    if (ticks === 2) {
      clearInterval(interval);
      resolve();
    }
  }, 1);
});

assert.deepEqual(events, ["timer-value", "interval-1", "interval-2"]);
console.log(JSON.stringify(events));
