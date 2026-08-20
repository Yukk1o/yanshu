# Yanshu General-Purpose Foundation v0.6

This document defines the Rust-first general-purpose language foundation exposed
by guest language `(version 2)`. The expense service is an acceptance program,
not the boundary of the language. Language version 1 remains supported without
receiving the new bindings or forms.

## Identity and repair protocol

Two identifiers have different jobs:

- `(version 1|2)` selects language semantics. Unknown versions fail with
  `PROGRAM_UNSUPPORTED_VERSION`; a v1 program using a v2 form fails with
  `PROGRAM_FEATURE_REQUIRES_VERSION`.
- SHA-256 of the complete UTF-8 source is the immutable program identity. The
  version store records this hash, the language version, parent hash, provider
  metadata and test report separately.

An automated repair loop consumes a fixed parent hash, complete source and
machine-readable failures; emits a complete candidate; parses and tests it; and
registers the resulting source under its new hash. Registration never means
promotion. Active state changes only through the test-gated promote operation.

## Version 2 expressions

- `(and expression ...)` evaluates left to right, stops at the first `#f`, and
  returns the selected operand. `(and)` returns `#t`.
- `(or expression ...)` evaluates left to right, stops at the first truthy
  value, and returns it. `(or)` returns `#f`.
- `(cond (condition expression) ... (else expression))` evaluates clauses in
  order. The explicit final `else` is mandatory and makes the branch table
  total and reviewable.

Only `#f` is false. These are special forms rather than primitives because
ordinary call arguments are evaluated eagerly.

## Version 2 schemas

- `(enum LITERAL ...)` accepts 1 to 64 unique integer, boolean or string
  literals.
- `(union SCHEMA SCHEMA ...)` accepts 2 to 8 variants.

Validation attempts union variants in source order. Failed branch issues are
discarded, but all attempted work is still charged to fuel. If no branch
matches, the stable issue is `SCHEMA_UNION`; enum mismatch uses `SCHEMA_ENUM`.

`validate` keeps the v1 `Ok(normalized) | Err(issues)` contract.
`validate-report` returns:

```json
{
  "valid": false,
  "value": "hold",
  "issues": [{"path": "/action", "code": "SCHEMA_ENUM"}],
  "cost": {"fuel": 3}
}
```

The exact issue object also contains a bounded public message and constructor
specific details. At most 32 issues are retained.

## Version 2 pure primitives

- `number->string : Int -> String` uses base-10 arbitrary-precision notation.
- `list-map : Function List -> List` preserves input length and order.
- `list-filter : Function List -> List` preserves the order of kept values.
- `list-fold : Function initial List -> Value` calls the function with
  `(accumulator item)` from left to right.
- `sum : List<Int> -> Int` returns zero for an empty list.

Every visited list item consumes fuel in addition to callback evaluation. Fuel
or call-depth exhaustion remains a system diagnostic.

## Recoverable business errors

`checked-quotient` and `checked-remainder` return `Ok(Int)` or an `Err` map with
stable business code `DIVIDE_BY_ZERO`. Argument type errors remain interpreter
diagnostics. The language deliberately has no general diagnostic catcher:
guest code cannot swallow fuel exhaustion, capability violations, persistence
failure or host failure.

## Reference program

`examples/expenses/service.yan` combines v2 boolean forms, list aggregation,
numeric key construction, enum/union schemas, quantified validation and
recoverable division. Its scenario suite is
`examples/expenses/scenarios.json`.

## General-purpose roadmap

1. v0.6 completes conditions, collections, schemas, Results and the expense
   acceptance service.
2. v0.7 adds modules, user-defined data, pattern matching and sealed bundles.
3. v0.8 adds types and effects with a statically computed capability closure.
4. v0.9 adds content-addressed packages, lockfiles and Rust library backends.
5. v0.10 adds fuel-metered bytecode and WebAssembly compilation.
6. Structured concurrency and a human-oriented surface syntax come only after
   those trust boundaries are stable.
