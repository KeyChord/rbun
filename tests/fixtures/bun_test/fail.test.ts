import { expect, test } from "bun:test";

test("reports native assertion failures", () => {
  expect("embedded").toBe("bun-cli");
});
