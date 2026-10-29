import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const dir = new URL("./", import.meta.url);
const { rbun } = JSON.parse(await readFile(new URL("package.json", dir), "utf8"));
const tag = `bun-embed-${rbun.bunRevision.slice(0, 12)}`;
const asset = "libbun_embed-aarch64-apple-darwin.dylib";
const base = `https://github.com/KeyChord/rbun/releases/download/${tag}`;

async function download(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(120_000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

const [binary, checksum, license] = await Promise.all([
  download(`${base}/${asset}`),
  download(`${base}/${asset}.sha256`),
  download(`https://raw.githubusercontent.com/oven-sh/bun/${rbun.bunRevision}/LICENSE.md`),
]);
const expected = checksum.toString().trim().split(/\s+/);
const actual = createHash("sha256").update(binary).digest("hex");
if (expected.length !== 2 || expected[1] !== asset || expected[0] !== actual) {
  throw new Error("Release asset SHA-256 does not match its checksum file");
}

// Reject a local-only binary or a mislabeled architecture before publication.
if (
  binary.readUInt32LE(0) !== 0xfeedfacf ||
  binary.readUInt32LE(4) !== 0x0100000c ||
  binary.readUInt32LE(12) !== 6
) {
  throw new Error("Expected a 64-bit ARM macOS dylib");
}
let minimumMacOS;
for (let offset = 32, i = 0; i < binary.readUInt32LE(16); i++) {
  const command = binary.readUInt32LE(offset);
  if (command === 0x32 && binary.readUInt32LE(offset + 8) === 1) {
    minimumMacOS = binary.readUInt32LE(offset + 12);
  }
  if (command === 0x24) minimumMacOS = binary.readUInt32LE(offset + 8);
  offset += binary.readUInt32LE(offset + 4);
}
const [major, minor = 0] = rbun.minimumMacOS.split(".").map(Number);
if (minimumMacOS === undefined || minimumMacOS > major * 65536 + minor * 256) {
  throw new Error(`Release asset requires a newer macOS than ${rbun.minimumMacOS}`);
}

await Promise.all([
  writeFile(new URL("libbun_embed.dylib", dir), binary, { mode: 0o755 }),
  writeFile(new URL("libbun_embed.dylib.sha256", dir), `${actual}  libbun_embed.dylib\n`),
  writeFile(new URL("BUN-LICENSE.md", dir), license),
]);
console.log(`Prepared ${fileURLToPath(dir)} from ${tag}; SHA-256 ${actual}`);
