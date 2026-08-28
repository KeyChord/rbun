import assert from "node:assert/strict";

const first = await import("./once.ts");
const second = await import("./once.ts");
const result = {
  evaluations: globalThis.__rbunCompatEvaluations,
  sameNamespace: first === second,
  values: [first.value, second.value],
};
assert.deepEqual(result, {
  evaluations: 1,
  sameNamespace: true,
  values: [1, 1],
});
console.log(JSON.stringify(result));

declare global {
  var __rbunCompatEvaluations: number | undefined;
}
