# Yanshu v0.9 package and Rust Library Backend contract

Status: implemented Rust host contract. Guest language version remains `4`; workspace release is `0.9.0`.

## Content-addressed packages

A source workspace has one `yanshu-package.source.json`. It names the package, a canonical `major.minor.patch` version, its entry module, local `.yan` module paths, and direct source dependencies. Dependency paths are development inputs only. They must be canonical relative paths contained by the root workspace.

Packing recursively parses every module and dependency, checks direct import ownership, and writes immutable artifacts below:

```text
<store>/sha256/<package-hash>/
  package.json
  <module>.yan
```

The package manifest contains exact module SHA-256 values and exact dependency package hashes. Its canonical compact JSON SHA-256 is the package hash. The artifact contains no install script, executable hook, Cargo manifest, dynamic library, registry URL, or development path.

Publishing uses a same-filesystem temporary directory followed by rename. An existing hash path is never overwritten; it is reverified byte-for-byte instead.

## Lockfile

`yanshu.lock.json` format 1 contains:

- the root package hash;
- entry module and language version;
- the statically recomputed capability closure;
- every package name, version, content hash, and exact direct dependency hashes, sorted by package name.

The loader treats the lockfile as a claim, not authority. It rereads every artifact, checks its path hash, manifest, source hashes, module identity, dependency identity, unique package and module names, language version, import closure, linked types/effects, and capability closure. It then reconstructs the canonical lock and requires exact equality. Development source paths are never consulted while loading or running a lock.

One package name maps to one hash in a lock closure. Multiple versions under the same name fail closed because the current module namespace does not encode package versions.

## Rust Library Backend

`yanshu-library` owns trusted contracts independently from the interpreter. `LibraryBackend` implementations provide:

- a bounded provider label;
- exact library name and version;
- an operation set that must exactly equal the trusted contract;
- safe Rust implementations over `LibraryValue` portable data.

The initial `RustTextBackend` implements `text@1`. The runtime installs library functions from the registered contract rather than matching hard-coded text primitive branches.

The host validates arguments and computes contract-owned fuel before invoking a backend. Insufficient fuel therefore prevents the backend call entirely. Backend errors and panics become `RUNTIME_LIBRARY_FAILURE`; backend diagnostics and panic text are not exposed. Results are checked against the contract and the existing depth, node, string, and portable-value limits.

Custom backends are supplied explicitly through `execute_export_with_libraries` or `execute_export_with_host_and_libraries`. Guest source cannot select a provider, crate, file, symbol, ABI, or dynamic library.

## CLI

```text
package-pack <workspace> <store>
package-lock <workspace> <store> <yanshu.lock.json>
package-verify <store> <content-hash>
package-inspect <store> <yanshu.lock.json>
package-review <store> <yanshu.lock.json> [--text]
package-run <store> <yanshu.lock.json> <export> <arguments.json>
```

`package-review --text` is still a one-way `rust-readonly-v1` projection. Locking and packages add no structured editor or reverse parser.

The name above records the v0.9 milestone. The current v0.10 renderer is `rust-readonly-v3`; it remains one-way and read-only.

## Security invariants

- no package installation or build scripts;
- no network resolution during lock loading or execution;
- no dependency fallback to development paths;
- no overwrite of an existing content-addressed artifact;
- no library operation before its contract fuel is charged;
- no backend-selected authority or capability;
- no first-party `unsafe`, C ABI, dynamic loading, `eval`, or ambient I/O.
