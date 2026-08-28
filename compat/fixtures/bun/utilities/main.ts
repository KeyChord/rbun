import assert from "node:assert/strict";

const result = {
  deepEqual: Bun.deepEquals(
    { a: [1, { b: new Set([2, 3]) }] },
    { a: [1, { b: new Set([2, 3]) }] },
  ),
  escaped: Bun.escapeHTML(`<div a="x">&'`),
  stringWidth: Bun.stringWidth("A😀界"),
};
assert.equal(result.deepEqual, true);
assert.equal(result.escaped, "&lt;div a=&quot;x&quot;&gt;&amp;&#x27;");
assert.equal(result.stringWidth, 5);
console.log(JSON.stringify(result));
