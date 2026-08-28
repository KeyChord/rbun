import assert from "node:assert/strict";

const hasher = new Bun.CryptoHasher("sha256");
hasher.update("r").update(new TextEncoder().encode("bun"));
const result = {
  algorithm: hasher.algorithm,
  digest: hasher.digest("hex"),
  staticHash: Bun.CryptoHasher.hash("sha256", "rbun", "hex"),
};
assert.deepEqual(result, {
  algorithm: "sha256",
  digest: "cdcd56ac0fd649e5591e4a3ac540978e31ec399be7a3356f6a747dbbb0e7144b",
  staticHash: "cdcd56ac0fd649e5591e4a3ac540978e31ec399be7a3356f6a747dbbb0e7144b",
});
console.log(JSON.stringify(result));
