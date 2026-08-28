import assert from "node:assert/strict";

const source = new ReadableStream<string>({
  start(controller) {
    controller.enqueue("embedded");
    controller.enqueue(" bun");
    controller.close();
  },
});
const transformed = source.pipeThrough(
  new TransformStream<string, string>({
    transform(chunk, controller) {
      controller.enqueue(chunk.toUpperCase());
    },
  }),
);
let text = "";
for await (const chunk of transformed) text += chunk;
assert.equal(text, "EMBEDDED BUN");
console.log(JSON.stringify({ text }));
