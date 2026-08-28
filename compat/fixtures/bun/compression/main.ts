import assert from "node:assert/strict";

const input = new TextEncoder().encode("embedded bun ".repeat(20));
const gzip = Bun.gzipSync(input);
const deflate = Bun.deflateSync(input);
const result = {
  deflateBytes: deflate.byteLength,
  deflateRoundtrip: new TextDecoder().decode(Bun.inflateSync(deflate)),
  gzipBytes: gzip.byteLength,
  gzipRoundtrip: new TextDecoder().decode(Bun.gunzipSync(gzip)),
};
assert.equal(result.deflateRoundtrip, "embedded bun ".repeat(20));
assert.equal(result.gzipRoundtrip, "embedded bun ".repeat(20));
assert(result.deflateBytes > 0 && result.deflateBytes < input.byteLength);
assert(result.gzipBytes > 0 && result.gzipBytes < input.byteLength);
console.log(JSON.stringify(result));
