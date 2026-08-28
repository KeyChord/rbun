import assert from "node:assert/strict";

const server = Bun.serve({
  port: 0,
  fetch(request) {
    const url = new URL(request.url);
    return Response.json({ method: request.method, path: url.pathname });
  },
});

try {
  const response = await fetch(new URL("/compat", server.url));
  const result = { body: await response.json(), status: response.status };
  assert.deepEqual(result, {
    body: { method: "GET", path: "/compat" },
    status: 200,
  });
  console.log(JSON.stringify(result));
} finally {
  await server.stop(true);
}
