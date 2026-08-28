import assert from "node:assert/strict";

const original = {
  big: 2n ** 80n,
  map: new Map([["answer", 42]]),
  set: new Set([3, 1, 3]),
  typed: new Uint16Array([0, 1, 65535]),
};
const cloned = structuredClone(original);
const result = {
  bigint: original.big.toString(),
  map: [...cloned.map],
  regexp: /(?<word>\p{Letter}+)/u.exec("héllo")?.groups?.word,
  set: [...cloned.set],
  typed: [...cloned.typed],
  unicode: [..."A😀é"],
};

assert.deepEqual(result, {
  bigint: "1208925819614629174706176",
  map: [["answer", 42]],
  regexp: "héllo",
  set: [3, 1],
  typed: [0, 1, 65535],
  unicode: ["A", "😀", "é"],
});
console.log(JSON.stringify(result));
