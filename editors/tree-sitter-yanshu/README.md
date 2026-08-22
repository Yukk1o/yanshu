# tree-sitter-yanshu

`tree-sitter-yanshu` provides incremental concrete syntax trees and standard
queries for `.yan` editor integrations. It covers Yanshu language versions
v1-v4, all three Reader list delimiters, comments, strings, quote data,
declarations, Schema/type forms, expressions, and patterns.

This parser is intentionally **display-only and error-tolerant**. It is not a
validator and must never be used for execution, content hashes, capability
analysis, sealing, promotion, or source rewriting. The safe-Rust Reader and
Parser in `yanshu-syntax` remain canonical; `.yan` source remains the only
executable input.

The generated native parser is not a sandbox and does not implement Yanshu
fuel or memory accounting. Editor adapters must reject oversized documents
before parsing; using the canonical Reader's 4 MiB source ceiling is the
recommended compatibility boundary.

## Development

```powershell
npm ci
npm run generate
npm test
```

`npm run check` additionally proves that `src/parser.c`, `src/grammar.json`,
and `src/node-types.json` match `grammar.js`. The test suite runs Tree-sitter
corpus and query checks, parses all valid repository `.yan` sources without
syntax errors, and asks the formatter's canonical Reader/Parser round trip to
accept the same source set. This deliberately avoids whole-program type/effect
analysis because an individual sealed Bundle module is not valid until linked.

Generated C is confined to this editor grammar package. There is deliberately
no first-party Rust binding: Yanshu's Rust crates remain `unsafe`-free and do
not link Tree-sitter into the language trust boundary.
