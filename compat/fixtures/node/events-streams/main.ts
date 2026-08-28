import assert from "node:assert/strict";
import { EventEmitter, once } from "node:events";
import { Readable } from "node:stream";

const emitter = new EventEmitter();
const received = once(emitter, "answer");
queueMicrotask(() => emitter.emit("answer", 40, 2));
const eventArgs = await received;

let streamed = "";
for await (const chunk of Readable.from(["node", " ", "stream"])) {
  streamed += chunk;
}
const result = { eventArgs, streamed };
assert.deepEqual(result, { eventArgs: [40, 2], streamed: "node stream" });
console.log(JSON.stringify(result));
