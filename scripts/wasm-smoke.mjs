import { readFile } from "node:fs/promises";

const artifactPath = process.argv[2];
if (!artifactPath) {
  throw new Error("usage: node scripts/wasm-smoke.mjs <artifact.wasm>");
}

const bytes = await readFile(artifactPath);
if (!WebAssembly.validate(bytes)) {
  throw new Error("WASM validation failed");
}

const module = await WebAssembly.compile(bytes);
const imports = WebAssembly.Module.imports(module).map(
  ({ module: namespace, name, kind }) => `${namespace}.${name}:${kind}`,
);
const exports = WebAssembly.Module.exports(module)
  .map(({ name, kind }) => `${name}:${kind}`)
  .sort();

const expectedImports = ["yanshu_v1.execute:function"];
const expectedExports = [
  "yanshu_format_version:function",
  "yanshu_run:function",
  "yanshu_static_instruction_weight:function",
].sort();

if (JSON.stringify(imports) !== JSON.stringify(expectedImports)) {
  throw new Error(`unexpected WASM imports: ${JSON.stringify(imports)}`);
}
if (JSON.stringify(exports) !== JSON.stringify(expectedExports)) {
  throw new Error(`unexpected WASM exports: ${JSON.stringify(exports)}`);
}

let invocations = 0;
const instance = await WebAssembly.instantiate(module, {
  yanshu_v1: {
    execute(exportIndex, argumentsHandle, fuel) {
      invocations += 1;
      if (exportIndex !== 0 || argumentsHandle !== 7 || fuel !== 1000n) {
        throw new Error("WASM handle ABI changed its arguments");
      }
      return 11n;
    },
  },
});

if (instance.exports.yanshu_format_version() !== 1) {
  throw new Error("unexpected WASM ABI format version");
}
if (instance.exports.yanshu_static_instruction_weight() <= 0n) {
  throw new Error("static instruction weight must be positive");
}
if (instance.exports.yanshu_run(0, 7, 1000n) !== 11n || invocations !== 1) {
  throw new Error("WASM handle ABI did not delegate exactly once");
}

console.log(
  JSON.stringify({
    ok: true,
    imports,
    exports,
    formatVersion: 1,
    invocations,
  }),
);
