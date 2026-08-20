# Yanshu v0.1 Design

## Portability boundary

Racket is the prototype host, not the guest language. Source is read as an
S-expression, validated, converted into explicit AST structs, and interpreted by
the project runtime. Guest code never reaches Racket `eval`.

The following artifacts are intended to survive a Rust migration unchanged:

- `.yan` source programs
- JSON test suites and reports
- diagnostic codes
- provider request and response shapes
- version metadata and event records
- language conformance tests

## Components

```text
source -> reader -> parser/AST -> evaluator -> value
                         |            |
                         |            +-> fuel/depth limits
                         +-> structured diagnostics

active version -> observations -> provider -> candidate
                                             |
                                  tests -> register -> promote
                                                        |
                                                     rollback
```

## Trust model

The reader, parser, evaluator, capability dispatcher, tests, version store, and
promotion policy form the trusted kernel. Provider output and guest source are
untrusted data.

The prototype exposes only pure primitives plus an optional `log` capability.
Execution is bounded by fuel and evaluator depth. A later production runtime
must additionally execute untrusted candidates in a separate OS process.

## Version model

Program source is addressed by SHA-256. Registering a candidate writes an
immutable source file and metadata. Promotion updates a small active pointer and
appends an audit event. Each request loads the active hash once and remains
pinned to that version for the request lifetime.

## Provider model

A provider accepts the current source and structured observations, then returns
a complete candidate source document. Returning a full document keeps the first
prototype small; stable AST patches can be added after the end-to-end loop is
validated.

The host-facing provider boundary is represented by `evolution-request`,
`evolution-proposal`, and `evolution-provider`. The bundled provider reads a
deterministic candidate file. A live LLM adapter should implement the same
procedure boundary and must remain outside the guest capability environment.
