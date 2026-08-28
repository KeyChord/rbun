const assert = require("node:assert/strict");
const path = require("node:path");
const dependency = require("./dependency.cjs");

const result = {
  basename: path.basename("/one/two/file.txt"),
  doubled: dependency.double(21),
  filename: path.basename(__filename),
};
assert.deepStrictEqual(result, {
  basename: "file.txt",
  doubled: 42,
  filename: "main.cjs",
});
console.log(JSON.stringify(result));
