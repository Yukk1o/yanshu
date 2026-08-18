# Host-Neutral Conformance Format v1

## Purpose

`conformance/v1/manifest.json` is the executable migration contract shared by
the Racket oracle and the Rust host. It tests observable language behavior,
not Racket function names or Rust implementation details.

The first manifest covers Reader/Parser failures, program metadata, recursion,
sequential `let`, truthiness, integers beyond `i64`, stable diagnostics, fuel,
Schema normalization/issues, and the `text@1` Library Contract.

## Case shape

Each case has a unique `name`, a `phase`, a source path relative to the
manifest, and an exact expected outcome:

```json
{
  "name": "recursive-factorial",
  "phase": "run",
  "source": "programs/core.ail",
  "entry": "factorial",
  "args": [{"$int": "5"}],
  "expect": {"kind": "value", "value": {"$int": "120"}}
}
```

Supported phases are `load`, `inspect`, and `run`. Successful load/inspect
cases compare a host-neutral program summary. Run cases compare a tagged guest
value or the complete stable diagnostic code, message, and details.

## Tagged guest values

JSON metadata numbers remain ordinary JSON numbers. Guest values use tags where
JSON would lose semantics:

| Guest value | Fixture encoding |
| --- | --- |
| arbitrary-precision Int | `{"$int":"9223372036854775808"}` |
| Nil / empty list | `{"$nil":true}` |
| Symbol | `{"$symbol":"name"}` |
| Ok | `{"$ok": VALUE}` |
| Err | `{"$err": VALUE}` |

Strings, booleans, non-empty lists, and string-keyed maps use their natural
JSON shape. Encoding every guest integer as a decimal string avoids accidental
`i64`/IEEE-754 narrowing in a host or JSON implementation.

## Runner requirements

A conforming runner must:

1. resolve source paths relative to the manifest rather than the process CWD;
2. apply per-case fuel and depth before guest execution;
3. support a deliberately empty library registry for authority tests;
4. return every case result instead of stopping at the first mismatch;
5. exit non-zero when any expected outcome differs;
6. never update expected outcomes automatically.

The Racket oracle is exposed through `conformance <manifest.json>`. The Rust
runner must consume this exact file and initially run beside the Racket runner;
changing the manifest format requires a new version directory.

