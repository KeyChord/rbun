import assert from "node:assert/strict";

const custom = new TypeError("outer", { cause: new Error("inner") });
const aggregate = new AggregateError([new Error("one"), 2], "many");
let importError: unknown;
try {
  await import("./missing-module.ts");
} catch (error) {
  importError = error;
}

assert(importError instanceof Error);
const result = {
  aggregate: {
    errors: aggregate.errors.map(error => error instanceof Error ? error.message : error),
    message: aggregate.message,
    name: aggregate.name,
  },
  custom: {
    cause: custom.cause instanceof Error ? custom.cause.message : null,
    message: custom.message,
    name: custom.name,
  },
  importError: {
    message: importError.message,
    name: importError.name,
  },
};
console.log(JSON.stringify(result));
