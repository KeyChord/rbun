import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, test } from "bun:test";

const events: string[] = [];

beforeAll(() => events.push("beforeAll"));
beforeEach(() => events.push("beforeEach"));
afterEach(() => events.push("afterEach"));
afterAll(() => {
  events.push("afterAll");
  expect(events).toEqual([
    "beforeAll",
    "beforeEach",
    "sync",
    "afterEach",
    "beforeEach",
    "async",
    "afterEach",
    "afterAll",
  ]);
});

describe("embedded bun:test", () => {
  test("runs Bun's native matcher implementation", () => {
    events.push("sync");
    expect(Bun.version).toBeString();
    expect(new Uint8Array([1, 2, 3])).toEqual(new Uint8Array([1, 2, 3]));
  });

  test("drives promises and timers", async () => {
    await Bun.sleep(1);
    events.push("async");
    expect(await Promise.resolve(42)).toBe(42);
  });
});
