# Changelog

## 0.12.0

- Ship the bounded safe-Rust `yanshu-lsp` in verified Windows x64 and Linux x64 VSIX assets.
- Add scope-aware completion, full semantic tokens, same-document definitions/references, and capture-avoiding rename.
- Add exact language/function/binding hover and the scriptless, version-bound Rust-style review panel.
- Build the LSP and VSIX twice per platform and bind them into the schema v3 release manifest, checksums, SBOMs, and keyless provenance.

## 0.10.0

- Register `.yan`, language configuration, and baseline TextMate highlighting.
- Start the bounded safe-Rust `yanshu-lsp` from a bundled binary, an absolute machine setting, or the host `PATH`.
- Add deterministic platform-specific VSIX packaging with a bounded non-symlink server input.
- Add isolated Windows/Linux Extension Host tests for activation, diagnostics, hover, definition, and formatting.
- Resolve parameter, sequential `let`, and pattern binding definitions with lexical shadowing.
- Find same-document references for global definitions and lexical local bindings without text search.
- Rename global and lexical bindings with versioned same-document edits and capture-avoiding symbol-graph validation.
- Complete visible forms, definitions, lexical bindings, primitives, constructors, schemas, and declared Library operations with exact edits and version/capability filtering.
- Open a scriptless, version-bound Rust-style review panel beside the canonical `.yan` editor.
- Show exact plaintext hover help for language forms, primitives, library operations, user functions, and lexical bindings.
