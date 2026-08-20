# AIL v0.8 type, effect, and review contract

Status: implemented Rust contract. Guest language version: `4`.

## Typed user data

Every v4 data field has an explicit type:

```lisp
(data decision
  (approved (amount integer))
  (review (amount integer) (reason string))
  (rejected (reason string)))

(export-types decision)
```

The supported type expressions are `any`, `integer`, `boolean`, `string`, `symbol`, `nil`, `map`, a local user-data type name, `(list TYPE)`, `(result SUCCESS ERROR)`, and `(fn (PARAMETER-TYPE ...) RESULT-TYPE)`. Type expressions and field counts remain bounded.

`export` and `export-types` are separate namespaces. A module may construct or match an imported variant only when its value constructor is in `export`; it may mention an imported nominal type in a signature only when that data type is in `export-types`. The Bundle linker resolves types from direct imports, qualifies their identity as `module/type`, and rejects missing or ambiguous providers with stable diagnostics. No ambient or transitive import is inferred.

## Export signatures

Every exported v4 definition has a function signature:

```lisp
(signature evaluate (fn (integer) map))
```

Constructors may be exported without a separate signature because their exact function type follows from the data declaration. A signature cannot name an unknown definition or type.

The checker seeds exported definitions from their signatures and infers internal definitions, closures, let bindings, calls, constructor patterns, collections, Results, Schemas, and the versioned primitive contracts. Static mismatch and arity diagnostics carry source spans.

`any` is an explicit gradual boundary for JSON maps, heterogeneous Schema unions, and dynamic lookups. It is not a claim that runtime data is safe: v4 checks host arguments before execution and checks the guest result against the export signature before returning it. Primitive runtime checks remain in force.

## Effect and capability closure

The effect checker computes the transitive set of capabilities reachable from each export. It follows:

- direct `log`, `clock`, and `kv` primitive calls;
- ordinary guest function calls and recursion;
- imported definitions after Bundle linking;
- inline and named callbacks passed to `list-map`, `list-filter`, and `list-fold`;
- callable parameters when their call site supplies a statically known callback.

An unresolved function parameter at an exported boundary produces `EFFECT_UNRESOLVED_PARAMETER`; the checker does not silently assume purity. A computed capability absent from `(capabilities ...)` produces `EFFECT_CAPABILITY_NOT_DECLARED`. Declared but unreachable capabilities are reported as `unusedCapabilities` so least-authority review is mechanical.

## Sealed capability closure

Language v4 uses Bundle manifest format 2. It adds one uniquely sorted `capabilityClosure` array. Sealing links and analyzes the full dependency closure before writing the manifest. Loading re-parses modules, re-links them, recomputes the closure, and rejects any mismatch. The field participates in the Bundle content hash.

Format 1 remains the exact v3 contract and cannot claim language v4. Format 2 is restricted to language v4.

## Runtime gate

The interpreter invokes static analysis for every v4 execution. Analysis is therefore not an optional CLI lint. It runs before capabilities are installed or guest definitions are evaluated.

The export boundary additionally enforces:

- exact argument arity;
- recursive input conformance for List and Result;
- nominal user-variant type identity;
- result conformance after execution, including values obtained through `any`.

## Rust-style read-only review

`review` and `review-bundle` generate `rust-readonly-v1`. The document contains:

`rust-readonly-v1` is the historical renderer identifier introduced by v0.8. The current v0.10 implementation emits `rust-readonly-v3`, which adds explicit effect-call and truthiness/BigInt annotations without making the projection editable.

- a permanent READ ONLY / non-executable header;
- typed enum-like user data;
- typed function signatures and inferred effects;
- Rust-style conditionals, matches, constructor fields, Maps, and calls;
- stable definition IDs, logical source module, source span, inferred type, and capability list in machine-readable nodes.

The review is not accepted as input and has `editable: false`. Structured editing is intentionally deferred until after v0.10; v0.8 adds no reverse parser and never treats generated review text as canonical source.

## Security invariant

Types do not create authority. Effect analysis may reduce or reject a declaration, but it cannot install a host capability. No type, signature, review node, or Bundle field introduces ambient I/O, dynamic loading, `eval`, FFI, or `unsafe` Rust.
