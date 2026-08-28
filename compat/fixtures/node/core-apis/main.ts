import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import path from "node:path";
import { gzipSync, gunzipSync } from "node:zlib";

const compressed = gzipSync(Buffer.from("embedded bun"));
const result = {
  buffer: Buffer.from([0, 1, 254, 255]).toString("base64"),
  hash: createHash("sha256").update("rbun").digest("hex"),
  path: path.posix.normalize("/a/b/../c//file.ts"),
  roundtrip: gunzipSync(compressed).toString(),
};
assert.deepEqual(result, {
  buffer: "AAH+/w==",
  hash: "cdcd56ac0fd649e5591e4a3ac540978e31ec399be7a3356f6a747dbbb0e7144b",
  path: "/a/c/file.ts",
  roundtrip: "embedded bun",
});
console.log(JSON.stringify(result));
