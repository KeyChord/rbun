import path from "node:path";

console.log(JSON.stringify({
  argvLength: process.argv.length,
  bunMainIsReferenceDriver: path.basename(Bun.main) === "reference-driver.mjs",
  bunMainIsRbunHost: path.basename(Bun.main) === "[rbun-host].js",
  execPathIsBun: path.basename(process.execPath) === "bun",
  importMetaMain: import.meta.main,
}));
