import assert from "node:assert/strict";
import config from "./config.json";
import { add, label } from "./math.ts";

interface Result {
  answer: number;
  label: string;
  nested: boolean;
}

const result: Result = {
  answer: await Promise.resolve(add(config.base, config.increment)),
  label,
  nested: config.nested.enabled,
};
assert.deepEqual(result, { answer: 42, label: "typescript", nested: true });
console.log(JSON.stringify(result));
