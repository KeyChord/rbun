import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { mode as imported } from "@rbun-compat/example";

const require = createRequire(import.meta.url);
const { mode: required } = require("@rbun-compat/example");
const result = { imported, required };
assert.deepEqual(result, { imported: "esm", required: "cjs" });
console.log(JSON.stringify(result));
