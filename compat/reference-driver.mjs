// The reference Bun imports fixtures instead of treating them as its CLI
// entry point, matching rbun-compat-host's production Module::import path.
const fixture = process.argv[2];
if (!fixture) {
  console.error("usage: bun reference-driver.mjs <fixture>");
  process.exitCode = 2;
} else {
  await import(Bun.pathToFileURL(fixture).href);
}
