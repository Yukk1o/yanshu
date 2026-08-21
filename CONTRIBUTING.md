# Contributing to Yanshu

Yanshu welcomes careful bug reports, conformance cases, documentation improvements, and narrowly scoped implementation changes. The project is experimental; correctness evidence matters more than feature count.

Before editing, read [docs/ai-agent-guide.md](docs/ai-agent-guide.md) in full. It is the shared contract for humans, Codex, Claude Code, OpenCode, and other repository agents.

## Non-negotiable rules

- Preserve the guest/host trust boundary and explicit capability model.
- Keep all first-party Rust safe; `unsafe` is forbidden in every form.
- Treat `.yan` source, sealed manifests, and locks as canonical.
- Keep version gates, interpreter/VM behavior, diagnostics, content hashes, and capability analysis synchronized.
- Never commit credentials or manually edit `wiki/public/source/`.

## Change workflow

1. Work on a focused branch.
2. Locate the specification, conformance case, and implementation path before editing.
3. Add the smallest failing test that expresses the intended contract.
4. Update every semantic surface affected by a language change.
5. Run targeted tests, then the release gates documented in the agent guide.
6. Explain security, fuel, compatibility, and generated-artifact consequences in the pull request.

Run `./scripts/check-repository-boundaries.ps1` for every Rust or workflow change. Changes to Reader, Parser, portable values, Bundle/package inputs, HTTP normalization, bytecode, or WASM loaders must also compile the independent fuzz workspace with `cargo check --locked --manifest-path fuzz/Cargo.toml --bins`; new crash artifacts must become minimized regression tests before the fix is considered complete.

Machine-readable diagnostic codes are compatibility surfaces. Improve messages when useful, but do not replace stable structure with prose that only a language model can interpret.

## Release changes

Changes to release workflows or packaging must also run:

```powershell
node --test scripts/release.test.mjs
node scripts/release-metadata.mjs
```

The tag-only release job is the sole publisher. Do not upload replacement
binaries by hand, weaken the annotated-tag/main/version checks, add a persistent
signing secret, or describe checksums alone as proof of authenticity. The full
artifact and provenance contract is in
[docs/engineering/release-supply-chain.md](docs/engineering/release-supply-chain.md).
