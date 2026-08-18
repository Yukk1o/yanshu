# Rust Dependency Audit

Audit checkpoint: 2026-08-18. The source of truth is the committed `Cargo.lock`;
this document explains the current decision, not a permanent approval of future
versions.

## Enforced boundary

All repository-owned crates inherit `unsafe_code = "forbid"` and also declare
`#![forbid(unsafe_code)]`. `scripts/check-rust.ps1` rejects first-party unsafe
blocks, unsafe functions, unsafe implementations, native ABI declarations, and
attempts to lower the lint. The compiler and Clippy then inspect every target.

Rust's standard library and several ecosystem crates contain internal unsafe
implementations. Workspace lints cannot propagate into them. Consequently the
enforceable invariant is: **no unsafe code in first-party crates; every external
implementation that contains unsafe is locked, inventoried, and reviewed before
a production release**. A dependency review never grants first-party code an
exception.

## Current graph

The current Rust host has two direct external dependencies:

| Dependency | Purpose | Decision |
| --- | --- | --- |
| `num-bigint 0.4.8` | Preserve Racket's arbitrary-precision integer semantics | Required until a smaller equivalently tested implementation exists |
| `serde_json 1.0.151` | Stable JSON values, diagnostics, and inspection output | Required for cross-host protocol compatibility |

Active transitive packages are `autocfg 1.5.1`, `itoa 1.0.18`, `memchr 2.8.3`,
`num-integer 0.1.47`, `num-traits 0.2.19`, `serde_core 1.0.229`, and
`zmij 1.0.23`. All come from crates.io; there are no git dependencies, wildcard
versions, or duplicated package versions. Declared licenses are covered by
MIT, Apache-2.0, and Unlicense.

The initial prototype briefly enabled Serde's derive feature. It was removed
before this checkpoint because the frontend does not need it, eliminating the
`proc-macro2`, `quote`, `syn`, `unicode-ident`, and `serde_derive` build chain.

The persisted KV implementation also evaluated `atomic-write-file 0.3.1`.
It was removed before this checkpoint because it added twelve packages to the
locked graph. The host instead creates a same-directory file with
`create_new`, writes and `sync_all`s it, then uses the safe standard-library
`rename` operation to replace the prior snapshot. This keeps atomic replacement
inside the supported Rust API without expanding the third-party trust base.

## Unsafe inventory status

Source inventory confirms internal unsafe implementations in `num-bigint`,
`serde_json`, `itoa`, `memchr`, `num-traits`, `serde_core`, and `zmij`.
`autocfg` and `num-integer` have no matching unsafe implementation in the
downloaded source. These internals are not callable as unsafe APIs from our
code, but they remain part of the trusted dependency base.

`cargo-deny 0.20.2` has executed successfully against this lockfile: advisories,
licenses, sources, duplicate versions, and wildcards all pass. This checkpoint
is suitable for frontend differential development, **not yet a production
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
