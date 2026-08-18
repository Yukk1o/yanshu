# Rust Host v0.5 Checkpoint

This checkpoint is the first executable Rust replacement path for the Racket
language host. It does not switch production traffic yet.

## Implemented

- `ail-diagnostic`: stable public diagnostics plus non-public source spans;
- `ail-syntax`: bounded UTF-8 Reader, AST, complete program/parser forms, schema
  metadata, and JSON inspection;
- `ail-runtime`: arbitrary-precision Values, lexical environments, recursive
  closures, sequential `let`, bounded evaluation, pure primitives, Schema
  normalization, and the `text@1` reference backend;
- `ail-conformance`: the same v1 JSON manifest/fixture codec used by Racket;
- `ail-store`: shared atomic file replacement plus content-addressed versions,
  immutable source verification, cross-process locking, promotion, and rollback;
- `ail-service`: explicit capability trait, route dispatch, response validation,
  transactional memory KV, Racket-compatible persisted file KV, fixed clock/log
  adapters, and scenario runner;
- `ail-http`: Axum/Tokio HTTP/1.1 adapter with bounded targets, headers, bodies,
  responses, concurrency and body-read deadlines; each request loads and pins one
  active program before execution;
- `ail-server`: independently deployable TCP process with structured startup and
  failure output plus graceful Ctrl+C shutdown;
- `ail-cli`: Rust `check`, `inspect`, `conformance`, `test-service`,
  `deploy-service`, and `version-conformance` commands.

The runtime represents environments and closures with checked arena indices.
It does not use pointers, native ABI shims, or unsafe self-referential structs.
Every first-party crate inherits workspace `unsafe_code = "forbid"` and repeats
`#![forbid(unsafe_code)]` at its crate root.

## Verified equivalence

Run:

```powershell
.\scripts\check-rust.ps1
```

The gate checks formatting, all Rust tests, Clippy with warnings denied,
first-party unsafe patterns, five full frontend comparisons, the complete
17-case conformance report, the 11-case task-service report, and a full
version-store promotion/rollback lifecycle. The Racket and Rust JSON documents
must be exactly equal after JSON normalization.

The HTTP crate additionally runs through a real loopback TCP socket in its test
suite, including active-version loading, HTTP/1.1 parsing, guest dispatch,
response encoding, connection close, and graceful server shutdown. Start the
complete Rust API path with:

```powershell
# terminal 1
.\scripts\serve-tasks-rust.ps1

# terminal 2
Invoke-RestMethod http://127.0.0.1:8081/tasks
```

The current corpus covers:

- program inspection and stable invalid-program diagnostics;
- recursion, lexical closures, sequential bindings, and truthiness;
- integers beyond `i64`, arithmetic errors, arity, fuel, and depth machinery;
- Schema defaults and stable ordered issues;
- exact-version Library Backend availability, types, fuel cost, Unicode text
  semantics, and result normalization;
- persisted KV reopen, invalid-document rejection, failed-write rollback, and
  temporary-file cleanup;
- Racket-compatible SHA-256 version IDs, metadata, event order, test-gated
  promotion, active pointers, restart reads, and rollback.

Supply-chain checks are separate because they may update the RustSec database:

```powershell
.\scripts\audit-rust.ps1
```

The current lockfile passes advisories, bans, licenses, and sources. Dependency
internal unsafe implementations remain inventoried according to
`docs/rust-dependency-audit.md`; they never permit unsafe in first-party code.

## Not switched yet

Racket remains the default browser service and semantic oracle. Rust now hosts
the capability/service boundary, compatible local persistence, version storage,
test-gated deployment, and an active-version JSON HTTP server. Static browser
assets and LLM providers have not migrated, and production still lacks auth,
metrics/tracing, database persistence, backups, and canary automation. The
cutover sequence remains: CI differential, offline replay, shadow execution,
canary, then explicit default-host change.
