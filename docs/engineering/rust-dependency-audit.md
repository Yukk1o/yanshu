# Rust Dependency Audit

Status: current dependency review record.

Audit checkpoint: 2026-08-18. The source of truth is the committed `Cargo.lock`;
this document explains the current decision, not a permanent approval of future
versions.

## Enforced boundary

All repository-owned crates inherit `unsafe_code = "forbid"` and also declare
`#![forbid(unsafe_code)]`. `scripts/check.ps1` rejects first-party unsafe
blocks, unsafe functions, unsafe implementations, native ABI declarations, and
attempts to lower the lint. The compiler and Clippy then inspect every target.

Rust's standard library and several ecosystem crates contain internal unsafe
implementations. Workspace lints cannot propagate into them. Consequently the
enforceable invariant is: **no unsafe code in first-party crates; every external
implementation that contains unsafe is locked, inventoried, and reviewed before
a production release**. A dependency review never grants first-party code an
exception.

## Current graph

The current Rust host has ten normal direct external dependencies and one
test-only dependency:

| Dependency | Purpose | Decision |
| --- | --- | --- |
| `num-bigint 0.4.8` | Preserve the language's arbitrary-precision integer semantics | Required until a smaller equivalently tested implementation exists |
| `num-traits 0.2.19` | Checked numeric conversions used at host boundaries | Small companion dependency of the arbitrary-precision value model |
| `serde_json 1.0.151` | Stable JSON values, diagnostics, and inspection output | Required for cross-host protocol compatibility |
| `sha2 0.11.0` | Preserve the established SHA-256 content identifiers | Exact-pinned RustCrypto implementation; default features disabled and portable software backend forced |
| `axum 0.8.9` | Route-independent HTTP/1.1 server adapter | Exact-pinned; default features disabled; only `http1` and `tokio` enabled |
| `tokio 1.53.1` | TCP runtime, bounded async body reads, signals, and graceful shutdown | Exact-pinned with a narrow explicit feature set |
| `reqwest 0.13.4` | Bounded HTTPS transport for live LLM providers | Exact-pinned; defaults disabled; only blocking control-plane calls and Rustls enabled |
| `zeroize 1.9.0` | Clear live provider credentials when their owner is dropped | Exact-pinned with only allocation support enabled |
| `getrandom 0.4.3` | Generate collision-resistant public HTTP request identifiers from the OS source | Exact-pinned low-level safe API; failure is explicit |
| `subtle 2.6.1` | Constant-time comparison of fixed-size Bearer token digests | Exact-pinned; raw tokens are hashed before comparison |
| `tower 0.5.3` | In-process HTTP router tests | Dev-only direct dependency; exact-pinned with only `util` enabled |

The host-target graph currently has 112 external packages; the union of enabled
normal, development, build, and platform edges across all targets has 151. HTTPS,
certificate verification, URL normalization, and their platform adapters account
for most of this increase. The exact list is machine-derived from `Cargo.lock` and
the Cargo graph; keeping a second hand-maintained transitive list here would go
stale. All packages come from crates.io and there are no git or wildcard
dependencies.

The graph is in an ecosystem transition where Reqwest's transitive URL/platform
derives still use `syn 2.0.119`, while current Tokio and other derives use
`syn 3.0.3`.
`deny.toml` skips exactly `syn@2.0.119` from duplicate rejection with a recorded
reason; every other active duplicate remains denied. The skip becomes an audit
warning when it no longer matches and must then be removed. Licenses are covered
by MIT, Apache-2.0, Unlicense, BSD-3-Clause, Unicode-3.0, and ISC, with one
version-specific CDLA-Permissive-2.0 exception for the WebPKI root-certificate
data package.

The syntax/runtime frontend still does not enable Serde derive. Axum's required
Tokio integration does, however, bring in the `tokio-macros`, `proc-macro2`,
`quote`, `syn`, and `unicode-ident` build chain. The host does not use procedural
macros in first-party source, but these locked build dependencies remain inside
the reviewed supply-chain boundary.

The live provider uses Reqwest's explicit Rustls backend and disables default
features, redirects, compression, cookies, HTTP/2, system proxy discovery, and
other unused client features. Rustls currently selects AWS-LC for cryptographic
operations and platform certificate verification. These implementations include
native and unsafe internals, but first-party code reaches them only through safe
Reqwest APIs. The Bearer value is not `Debug`, never enters provider diagnostics,
and is held in `Zeroizing<String>`.

The persisted KV implementation also evaluated `atomic-write-file 0.3.1`.
It was removed before this checkpoint because it added twelve packages to the
locked graph. The host instead creates a same-directory file with
`create_new`, writes and `sync_all`s it, then uses the safe standard-library
`rename` operation to replace the prior snapshot. This keeps atomic replacement
inside the supported Rust API without expanding the third-party trust base.

`yanshu-ops` adds the offline backup/verify/restore boundary without adding any
external package. It reuses the locked `sha2` implementation, standard-library
exclusive file locks, `create_new` writes, bounded directory traversal, and the
existing semantic validators. Restore has no overwrite flag.

`yanshu-rollout` also adds no external package. Deterministic sampling and redacted
body/header fingerprints reuse the locked `sha2`; candidate execution reuses the
existing bounded interpreter and an isolated in-memory KV snapshot.

The version store uses the ecosystem implementation instead of a handwritten
SHA-256 primitive. `sha2` is exact-pinned to 0.11.0, its unused `alloc` and
`oid` defaults are disabled, and `.cargo/config.toml` selects the documented
portable software backend. This keeps version hashes identical across target
CPUs and excludes the crate's optional instruction-specific backend paths from
the build. The full downloaded sources remain in the inventory even where a
target or cfg makes a site unreachable.

## Unsafe inventory status

Source inventory confirms no matching unsafe implementation in `axum`,
`axum-core`, `tower`, `tower-layer`, or `tower-service`. Lower transport/runtime
dependencies including Tokio, Hyper, Mio, Socket2, Bytes, and platform support
do contain internal unsafe implementations. Reqwest has a small number of
internal unsafe sites; Rustls itself has no matching sites, while AWS-LC and its
sys crate form the audited native cryptographic boundary. The script prints a per-package
matching-line count for the complete resolved graph. This is an inventory aid,
not proof that every match is reachable or correct; those internals remain part
of the trusted dependency base even though no unsafe API is called by
first-party code.

`cargo-deny 0.20.2` has executed successfully against this lockfile: advisories,
licenses, sources, the documented exact duplicate exception, and wildcards all
pass. This checkpoint
is suitable for cross-host differential development, **not yet a production
dependency approval**. Before a production release we must:

1. run `cargo deny check` against the locked graph and fail on advisories,
   yanked crates, disallowed sources, licenses, wildcards, or duplicates;
2. record the exact enabled features and review reachable unsafe sites for the
   target triples we ship;
3. build and test from a clean locked dependency cache;
4. repeat the review whenever `Cargo.lock` changes.

`deny.toml` commits the machine-enforced advisory, license, duplicate, wildcard,
and source policy. Run `scripts/audit-rust.ps1` to repeat those checks and print
the locked dependency unsafe implementation inventory.
