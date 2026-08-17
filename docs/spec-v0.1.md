# AI-Evolve Language Specification v0.1

## Program

```lisp
(program
  (name discount)
  (version 1)
  (capabilities)
  (def calculate-discount
    (fn (price user-type)
      (if (= user-type "vip")
          (- price (quotient price 10))
          price)))
  (export calculate-discount))
```

A document contains exactly one proper S-expression. Allowed source atoms are
exact integers, booleans, strings, and symbols.

## Forms

- `(quote datum)` returns inert data.
- `(if condition consequent alternative)` treats only `#f` as false.
- `(let ((name expression) ...) body)` evaluates bindings left to right; each
  later binding can reference earlier bindings in the same form.
- `(fn (parameter ...) body)` creates a lexical closure.
- `(do expression ...)` evaluates expressions in order and returns the last.
- Any other non-empty list is a function call.

Top-level forms are `name`, `version`, `capabilities`, `def`, and `export`.
Names and definitions must be unique. Every export must name a definition.
`version` identifies the guest-language version, while deployed program versions
are identified independently by their source hash.

## Values

`Nil`, `Bool`, `Int`, `String`, `Symbol`, `List`, `Map`, `Ok`, `Err`, closures,
and trusted primitives. Closures and primitives cannot be serialized as output.

## Pure primitives

Arithmetic and comparison: `+`, `-`, `*`, `quotient`, `remainder`, `=`, `<`,
`<=`, `>`, `>=`, `not`.

Collections: `list`, `empty?`, `length`, `first`, `rest`, `map`, `get`, `assoc`.

Results: `ok`, `err`, `ok?`, `err?`, `unwrap`.

The effectful `log` primitive is present only when the program declares the
`log` capability.

## Limits

Reader limits bound source node count and nesting depth. Evaluator limits bound
evaluation steps (fuel) and call depth. Exhaustion produces a structured error;
it is never reported as an ordinary guest value.

## Diagnostics

Every language error has a stable code, message, and details object. CLI
commands emit JSON so a future LLM provider can repair a program without parsing
human-oriented terminal text.

## Tests

A test suite supplies an exported entry point and cases with `args` and
`expect`. A candidate can be promoted only when its complete test report passes.
