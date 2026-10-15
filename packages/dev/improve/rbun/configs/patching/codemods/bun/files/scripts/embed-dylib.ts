/**
 * Link `libbun_embed.dylib` — Bun as an embeddable shared library (Chord's
 * `rbun` experiment).
 *
 * Reuses the exact object/archive list of the executable link edge that
 * `bun run build --profile=release` wrote into `build/release/build.ninja`
 * (so the C++ objects, Rust runtime, WebKit and every dependency
 * archive are byte-identical to the CLI build) and relinks them as a dylib:
 *
 *   - `-dynamiclib` + `@rpath` install name
 *   - executable-only flags dropped (`-stack_size`, order file, linker map,
 *     the NAPI/V8 `symbols.txt` export list)
 *   - the `libbun_runtime` archive force-loaded so `bun_embed_*` survive archive
 *     extraction (nothing in the C++ side references them)
 *   - every JavaScriptCore public C API symbol (`_JS*`, unmangled) pulled
 *     in with `-u` and exported, plus `_bun_embed_*` and `_napi_*`/`_node_*`
 *     from `src/symbols.txt` so `require("*.node")` keeps working
 *
 * Usage: bun scripts/embed-dylib.ts [--build-dir=build/release] [--out=libbun_embed.dylib]
 */

import { dirname, isAbsolute, join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");

const args = new Map<string, string>();
for (const arg of process.argv.slice(2)) {
  const m = /^--([^=]+)=(.*)$/.exec(arg);
  if (!m) throw new Error(`unknown argument ${arg}`);
  args.set(m[1], m[2]);
}
const buildDir = resolve(repo, args.get("build-dir") ?? "build/release");
const outName = args.get("out") ?? "libbun_embed.dylib";
const outPath = resolve(buildDir, outName);

const ninja = await Bun.file(join(buildDir, "build.ninja")).text();

// ─── Locate the executable link edge ───
// `build <exe> | <map>: link a.o b.o $\n    c.o ... $\n    lib.a\n  ldflags = ...`
// Ninja can wrap the output list and the rule name as well as the inputs.
const lines = ninja.replace(/\$\r?\n[ \t]*/g, "").split(/\r?\n/);
const start = lines.findIndex(l => /^build bun(-profile|-debug|-asan)?( \||:)/.test(l) && l.includes(": link "));
if (start < 0) throw new Error("could not find the bun link edge in build.ninja");
const edge = lines[start];
let ldflags = "";
for (let j = start + 1; j < lines.length && lines[j].startsWith("  "); j++) {
  const m = /^  ldflags = (.*)$/.exec(lines[j]);
  if (m) ldflags = m[1];
}
const inputsText = edge.slice(edge.indexOf(": link ") + ": link ".length);
// Explicit inputs only (before implicit, order-only, or validation inputs).
const inputs = inputsText
  .split(/\s+/)
  .filter(Boolean)
  .filter((t, idx, arr) => {
    const bar = arr.findIndex(x => x === "|" || x === "||" || x === "|@");
    return bar < 0 || idx < bar;
  })
  .map(t => t.replace(/\$ /g, " "))
  .map(t => (isAbsolute(t) ? t : resolve(buildDir, t)));

const rustLib = inputs.find(p => /\/libbun_runtime(?:-[a-f0-9]+\.rlib|\.a)$/.test(p));
if (!rustLib) throw new Error("libbun_runtime archive not among link inputs");
const jscLib = inputs.find(p => p.endsWith("libJavaScriptCore.a"));
if (!jscLib) throw new Error("libJavaScriptCore.a not among link inputs");
const linkInputs = inputs.filter(p => p !== rustLib);

// ─── Flags ───
const dropNext = new Set(["-exported_symbols_list"]);
const keptFlags: string[] = [];
const rawFlags = ldflags.split(/\s+/).filter(Boolean);
for (let k = 0; k < rawFlags.length; k++) {
  const f = rawFlags[k];
  if (dropNext.has(f)) {
    k++;
    continue;
  }
  if (/^-Wl,-(stack_size|order_file|map),/.test(f)) continue;
  keptFlags.push(f);
}

const cxxMatch = /clang\+\+/.test(ninja) ? /(\S*clang\+\+)/.exec(ninja)?.[1] : undefined;
const cxx = process.env.CXX ?? cxxMatch ?? "clang++";

// ─── Exported symbols ───
// Use the matching LLVM tools to read WebKit's LTO bitcode.
const llvmNm = join(dirname(cxx), "llvm-nm");
const nmCommand = process.env.NM ?? ((await Bun.file(llvmNm).exists()) ? llvmNm : "nm");
const nm = Bun.spawnSync([nmCommand, "-gU", jscLib], {
  stdin: "ignore",
  stdout: "pipe",
  stderr: "pipe",
  maxBuffer: 1 << 28,
});
if (nm.exitCode !== 0) throw new Error(`nm failed: ${nm.stderr.toString()}`);
const jscApi = new Set<string>();
for (const line of nm.stdout.toString().split("\n")) {
  // LLVM bitcode symbols have no address and llvm-nm prints dashes instead.
  const m = /^(?:[0-9a-f]+|-+) T (_JS[A-Z][A-Za-z0-9]*)$/.exec(line);
  if (m) jscApi.add(m[1]);
}
if (jscApi.size === 0) throw new Error("no JavaScriptCore C API symbols found in libJavaScriptCore.a");
const embedSyms = [
  "_bun_embed_run_internal_process_mode",
  "_bun_embed_init",
  "_bun_embed_vm_create",
  "_bun_embed_test_vm_create",
  "_bun_embed_test_run_file",
  "_bun_embed_vm_global_object",
  "_bun_embed_vm_configure_entrypoint",
  "_bun_embed_vm_run_eval",
  "_bun_embed_vm_tick",
  "_bun_embed_vm_drain_microtasks",
  "_bun_embed_vm_is_event_loop_alive",
  "_bun_embed_vm_auto_tick_active",
  "_bun_embed_vm_run_until_idle",
  "_bun_embed_vm_finish_process",
  "_bun_embed_vm_wait_for_promise",
  "_bun_embed_promise_status",
  "_bun_embed_promise_result",
  "_bun_embed_promise_set_handled",
  "_bun_embed_vm_garbage_collect",
  "_bun_embed_vm_wakeup",
  "_bun_embed_vm_delete_module_registry_entry",
  "_bun_embed_vm_set_import_meta_callback",
  "_bun_embed_last_error",
];
const napiSyms = (await Bun.file(join(repo, "src/symbols.txt")).text())
  .split("\n")
  .map(s => s.trim())
  .filter(s => s && !s.startsWith("#") && s !== "__mh_execute_header");
const exports = [...embedSyms, ...[...jscApi].sort(), ...napiSyms];
const exportsFile = join(buildDir, "embed-exports.txt");
await Bun.write(exportsFile, exports.join("\n") + "\n");

// ─── Link ───
const rsp = join(buildDir, `${outName}.rsp`);
await Bun.write(rsp, linkInputs.join("\n") + "\n");

const cmd = [
  cxx,
  `@${rsp}`,
  "-Wl,-force_load",
  rustLib,
  ...keptFlags,
  "-dynamiclib",
  "-install_name",
  `@rpath/${outName}`,
  "-exported_symbols_list",
  exportsFile,
  // Pull the JSC C API objects out of libJavaScriptCore.a even though
  // nothing in bun references them.
  ...[...jscApi].flatMap(s => [`-Wl,-u,${s}`]),
  "-o",
  outPath,
];
console.log(`[embed-dylib] linking ${outPath} (${linkInputs.length} inputs, ${exports.length} exports)`);
const t0 = performance.now();
const res = Bun.spawnSync(cmd, {
  stdin: "inherit",
  stdout: "inherit",
  stderr: "inherit",
  cwd: dirname(outPath),
});
if (res.exitCode !== 0) {
  console.error(`[embed-dylib] link failed (status ${res.exitCode})`);
  process.exit(res.exitCode || 1);
}
console.log(`[embed-dylib] done in ${((performance.now() - t0) / 1000).toFixed(1)}s`);
if (!(await Bun.file(outPath).exists())) process.exit(1);
