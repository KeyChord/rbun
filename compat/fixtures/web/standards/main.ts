import assert from "node:assert/strict";

const headers = new Headers({ "X-Answer": "42", Accept: "application/json" });
const response = new Response(JSON.stringify({ ok: true }), {
  headers: { "content-type": "application/json" },
  status: 201,
});
const fetched = await fetch("data:text/plain;charset=utf-8,hello%20rbun");
const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("rbun"));
const result = {
  digest: Buffer.from(digest).toString("hex"),
  fetched: await fetched.text(),
  header: headers.get("x-answer"),
  json: await response.json(),
  status: response.status,
  url: new URL("../c?x=1", "https://example.com/a/b/").href,
};
assert.deepEqual(result, {
  digest: "cdcd56ac0fd649e5591e4a3ac540978e31ec399be7a3356f6a747dbbb0e7144b",
  fetched: "hello rbun",
  header: "42",
  json: { ok: true },
  status: 201,
  url: "https://example.com/a/c?x=1",
});
console.log(JSON.stringify(result));
