# Library Backend Specification v0.4

## Objective

Allow guest programs to use a small, versioned standard-library contract while
the host selects the implementation. The same `.ail` source must be able to run
against a Racket reference backend now and a Rust, isolated Python, or
WebAssembly backend later without exposing arbitrary host functions.

This checkpoint validates the production boundary, not a package manager. A
guest cannot name a crate, PyPI package, shared library, file path, or backend
provider.

## Program declaration

A program declares each standard library and major contract version once:

```lisp
(program
  (name text-demo)
  (version 1)
  (capabilities)
  (libraries (text 1))
  (def inspect
    (fn (value)
      (map "length" (text/length value)
           "hasPrefix" (text/starts-with? value "AI-"))))
  (export inspect))
```

`libraries` is optional and defaults to an empty list. A declaration is
compiler-owned metadata, not an expression. Names are bounded lowercase
identifiers, versions are bounded positive integers, and a program cannot
declare two versions of the same library. Unknown contracts fail during
parsing.

Library functions use `<library>/<operation>` names. A schema or guest
definition cannot occupy a declared library namespace. Undeclared library
functions remain unbound.

## Contract, backend, and capability are different

```text
guest / portable std code
          |
          v
versioned library contract  -- names, arity, types, cost, result boundary
          |
          v
capability policy           -- authority such as network, file, database
          |
          v
selected backend            -- Racket, Rust, Python sidecar, WASM component
```

The trusted contract owns the public function set, argument and result kinds,
and fuel estimator. A backend supplies only implementations. Extra or missing
functions make the backend invalid. This prevents a backend from silently
adding ambient authority or claiming that expensive work is free.

`libraries` does not grant authority. The initial `text@1` contract is pure. A
future `http@1` contract would additionally require an explicit network
capability with host-owned URL, deadline, byte, and call-count policy.

## Backend registry

The host registry is keyed by `(library name, contract version)`. Each backend
records its own name, version, bounded provider label, and exact implementation
map. Guest source cannot observe or select the provider label.

The v0.4 reference registry contains `text@1` implemented by Racket:

| Function | Arguments | Result | Semantics |
| --- | --- | --- | --- |
| `text/length` | `String` | `Int` | Unicode scalar-value count |
| `text/starts-with?` | `String String` | `Bool` | literal prefix test |
| `text/ends-with?` | `String String` | `Bool` | literal suffix test |
| `text/contains?` | `String String` | `Bool` | literal substring test |

Rust conformance implementations must use character count rather than UTF-8
byte length for `text/length`.

## Runtime boundary

Every library call:

1. resolves a declared contract and an exact-version backend;
2. checks the contract-owned arity and argument kinds;
3. charges contract-owned fuel proportional to bounded input work;
4. calls the selected implementation behind a host exception boundary;
5. checks and normalizes the result into immutable portable guest data;
6. rejects excessive result depth, node count, or string size.

Expected guest input errors use stable language diagnostics. An unexpected
backend exception becomes `RUNTIME_LIBRARY_FAILURE`; its public details contain
the library, version, operation, and provider but never the host exception text.
Invalid backend shape or output uses `RUNTIME_INVALID_LIBRARY_BACKEND` or
`RUNTIME_LIBRARY_INVALID_RESULT`.

No raw pointer, socket, file handle, database connection, process object,
credential, host closure, or mutable container may cross the value boundary.
Future stateful resources use opaque, host-owned handles with request lifetime
and capability checks.

## Standard-library layering

The long-term layering follows this shape:

```text
std/       portable modules written in AI-Evolve where practical
host/      versioned contracts and capability-aware wrappers
backend/   Rust native adapters, Python sidecars, or WASM components
```

The initial text operations are installed directly from a backend because the
language does not yet have a module loader. Adding modules must not bypass this
contract boundary.

## Comments and generated documentation

LLM providers should generate concise comments for non-obvious invariants and
explain why a constraint exists instead of restating syntax. Comments are not
an acceptance gate because fluent but false comments are worse than missing
comments.

The current Racket reader preserves the complete source artifact but discards
comments before AST construction. A later lossless CST must retain comment
trivia and source spans so a review renderer can map `;` to Rust `//`, document
comments to `///`, and block comments to `/* ... */` without guessing which AST
node they describe.

## Acceptance

1. `libraries` declarations are bounded, inspectable, and reject unknown or
   duplicate contracts and namespace collisions.
2. Undeclared, unavailable, wrong-version, missing-operation, and extra-operation
   cases fail with stable diagnostics.
3. `text@1` executes through the reference backend and another conforming test
   backend can replace it without changing guest source.
4. Contract arity/types, fuel cost, backend exception redaction, and portable
   result limits are covered by tests.
5. Pure-function and service execution paths use the same backend registry.
6. Existing language, Web, evolution, persistence, and rollback tests remain
   green.

