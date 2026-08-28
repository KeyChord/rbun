import assert from "node:assert/strict";
import { domainToASCII, domainToUnicode } from "node:url";
import { promisify } from "node:util";

const callbackAdd = (left: number, right: number, cb: (error: null, value: number) => void) =>
  cb(null, left + right);
const add = promisify(callbackAdd);
const parsed = new URL("https://user:pass@example.com:8443/a?x=1#hash");
const result = {
  ascii: domainToASCII("münich.example"),
  formatted: parsed.href,
  sum: await add(40, 2),
  unicode: domainToUnicode("xn--mnich-kva.example"),
};
assert.deepEqual(result, {
  ascii: "xn--mnich-kva.example",
  formatted: "https://user:pass@example.com:8443/a?x=1#hash",
  sum: 42,
  unicode: "münich.example",
});
console.log(JSON.stringify(result));
