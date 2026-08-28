import assert from "node:assert/strict";
import { fromA, readA } from "./b.ts";
import { fromB, readB } from "./a.ts";

const result = { fromA, fromB, readA: readA(), readB: readB() };
assert.deepEqual(result, {
  fromA: "a",
  fromB: "b",
  readA: "b",
  readB: "a",
});
console.log(JSON.stringify(result));
