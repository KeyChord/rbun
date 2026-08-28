import assert from "node:assert/strict";
import { Database } from "bun:sqlite";

const db = new Database("compat.sqlite", { create: true, strict: true });
try {
  db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL)");
  const insert = db.prepare("INSERT INTO items (name, score) VALUES ($name, $score)");
  insert.run({ name: "alpha", score: 1.5 });
  insert.run({ name: "beta", score: 2.25 });
  const rows = db.query("SELECT id, name, score FROM items ORDER BY id").all();
  assert.deepEqual(rows, [
    { id: 1, name: "alpha", score: 1.5 },
    { id: 2, name: "beta", score: 2.25 },
  ]);
  console.log(JSON.stringify(rows));
} finally {
  db.close();
}
