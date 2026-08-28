/** @jsxRuntime classic */
/** @jsx h */
import assert from "node:assert/strict";

type Child = string | number | Node;
type Node = { tag: string; props: Record<string, unknown>; children: Child[] };

function h(tag: string, props: Record<string, unknown> | null, ...children: Child[]): Node {
  return { tag, props: props ?? {}, children };
}

const tree = <section id="answer"><strong>{40 + 2}</strong>ok</section>;
assert.deepEqual(tree, {
  tag: "section",
  props: { id: "answer" },
  children: [{ tag: "strong", props: {}, children: [42] }, "ok"],
});
console.log(JSON.stringify(tree));
