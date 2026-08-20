# Migrating to Yanshu

The v0.10 public launch replaces the experimental project name **AI-Evolve / AIL** with **Yanshu（衍术）**. This is an intentionally breaking, pre-stability migration; the repository does not retain executable aliases for the old namespace.

| Before | Yanshu |
|---|---|
| project name `AI-Evolve` | `Yanshu（衍术）` |
| `.ail` source | `.yan` source |
| `ail-*` crates | `yanshu-*` crates |
| `ail_*` Rust modules | `yanshu_*` Rust modules |
| `ail-cli` package | `yanshu-cli` package; binary `yanshu` |
| `AI_EVOLVE_*` environment variables | `YANSHU_*` environment variables |
| `ail-package.source.json` | `yanshu-package.source.json` |
| `ail.lock.json` | `yanshu.lock.json` |
| `.aibc.json` bytecode envelope | `.ybc.json` bytecode envelope |
| `.ail-store.lock` | `.yanshu-store.lock` |
| `ail_v1` WASM import namespace | `yanshu_v1` |
| `ail_run` WASM export | `yanshu_run` |
| `ail.meta.v1` / `ail.bytecode.v1` | `yanshu.meta.v1` / `yanshu.bytecode.v1` |

Canonical manifest paths, compiler targets, WASM custom sections, and ABI names changed. Consequently, package hashes, Bundle hashes, compiled artifact hashes, and persisted code-store identities may change even when expression semantics do not.

Do not point the renamed runtime at the only copy of an older experimental store. Keep the old directory as a backup, create a fresh runtime directory, reseal Bundles, regenerate package locks, recompile artifacts, and rerun conformance before promoting any source.

The untracked `.runtime/` directory is intentionally not rewritten by the repository migration.
