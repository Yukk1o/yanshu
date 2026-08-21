# Rust Safety Policy

Status: current and normative for all first-party Rust.

## First-party code

Every Rust crate maintained in this repository must contain:

```rust
#![forbid(unsafe_code)]
```

The workspace also sets the `unsafe_code` lint to `forbid`, and every member
inherits workspace lints. `forbid` is intentional: a nested module cannot
override it with `allow`.

First-party code must not contain unsafe blocks, unsafe functions, unsafe trait
implementations, raw-pointer dereferences, hand-written native ABI shims, or
build-time generated unsafe source. FFI-style library access uses safe crate
APIs behind the versioned Library Contract, an isolated process protocol, or a
WebAssembly Component interface.

## Dependencies

Rust lints do not apply to dependency crates, and the Rust standard library and
some ecosystem crates use audited unsafe internally. Therefore “zero unsafe in
first-party code” is an enforceable compile-time invariant, while dependencies
need a supply-chain policy:

- dependencies are pinned by `Cargo.lock`;
- default to dependencies with no unsafe implementation when practical;
- run advisory, license, source, and duplicate-version checks in CI;
- inventory dependency unsafe usage;
- require a documented review before accepting a dependency that contains
  unsafe code;
- prohibit unaudited git/path dependencies in release builds.

An exception for a dependency never permits unsafe code in a repository-owned
crate. The production release gate fails if first-party unsafe is detected or a
dependency violates the committed policy.
