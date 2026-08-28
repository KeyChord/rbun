import assert from "node:assert/strict";

const worker = new Worker(new URL("./worker.ts", import.meta.url).href);
try {
  const result = await new Promise<unknown>((resolve, reject) => {
    worker.onmessage = event => resolve(event.data);
    worker.onerror = reject;
    worker.postMessage({ left: 40, right: 2 });
  });
  assert.deepEqual(result, { answer: 42, kind: "worker" });
  console.log(JSON.stringify(result));
} finally {
  worker.terminate();
}
