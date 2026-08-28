import assert from "node:assert/strict";

const result = {
  cwd: process.cwd(),
  env: process.env.RBUN_COMPAT_FIXED,
  importUrl: import.meta.url,
  platform: process.platform,
};
assert.equal(result.env, "fixed-value");
assert(result.importUrl.endsWith("/main.ts"));
console.log(JSON.stringify(result));
