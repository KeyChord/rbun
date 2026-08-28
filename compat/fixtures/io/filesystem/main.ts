import assert from "node:assert/strict";
import { appendFile, readFile, stat } from "node:fs/promises";

const written = await Bun.write("output.txt", "alpha");
await appendFile("output.txt", "\nbeta\n");
const contents = await readFile("output.txt", "utf8");
const info = await stat("output.txt");
const result = {
  bunFileText: await Bun.file("output.txt").text(),
  contents,
  isFile: info.isFile(),
  size: info.size,
  written,
};
assert.deepEqual(result, {
  bunFileText: "alpha\nbeta\n",
  contents: "alpha\nbeta\n",
  isFile: true,
  size: 11,
  written: 5,
});
console.log(JSON.stringify(result));
