import assert from "node:assert/strict";

const file = new File(["hello", new Uint8Array([32, 114, 98, 117, 110])], "greeting.txt", {
  lastModified: 123456789,
  type: "text/plain;charset=utf-8",
});
const form = new FormData();
form.set("answer", "42");
form.set("file", file);
const stored = form.get("file");
assert(stored instanceof File);
const result = {
  answer: form.get("answer"),
  lastModified: stored.lastModified,
  name: stored.name,
  size: stored.size,
  text: await stored.text(),
  type: stored.type,
};
assert.deepEqual(result, {
  answer: "42",
  lastModified: 123456789,
  name: "greeting.txt",
  size: 10,
  text: "hello rbun",
  type: "text/plain;charset=utf-8",
});
console.log(JSON.stringify(result));
