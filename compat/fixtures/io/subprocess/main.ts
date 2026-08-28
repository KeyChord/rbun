import assert from "node:assert/strict";

const process = Bun.spawn(
  ["/bin/sh", "-c", "printf '%s' \"$RBUN_CHILD_VALUE\"; printf '%s' err >&2"],
  {
    env: { ...Bun.env, RBUN_CHILD_VALUE: "child-output" },
    stderr: "pipe",
    stdout: "pipe",
  },
);
const [exitCode, stdout, stderr] = await Promise.all([
  process.exited,
  new Response(process.stdout).text(),
  new Response(process.stderr).text(),
]);
const result = { exitCode, stderr, stdout };
assert.deepEqual(result, { exitCode: 0, stderr: "err", stdout: "child-output" });
console.log(JSON.stringify(result));
