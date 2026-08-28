import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<{ requestId: number }>();
const values = await storage.run({ requestId: 42 }, async () => {
  const before = storage.getStore()?.requestId;
  await Bun.sleep(1);
  const after = storage.getStore()?.requestId;
  return { after, before };
});
assert.deepEqual(values, { after: 42, before: 42 });
assert.equal(storage.getStore(), undefined);
console.log(JSON.stringify(values));
