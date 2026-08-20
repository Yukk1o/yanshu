# Yanshu agent entry

Before changing this repository, read [`docs/ai-agent-guide.md`](docs/ai-agent-guide.md) in full. It is the shared contract for Codex and other repository agents.

The non-negotiable rules are:

- Preserve the language trust boundary: no guest `eval`, implicit host access, unmetered work, or undeclared capability.
- First-party Rust must remain safe Rust. `unsafe` code, blocks, traits, functions, and implementations are forbidden.
- Treat `.yan` source and sealed manifests as canonical. Rust-style review output is generated, read-only, and never executable input.
- Keep language-version gates, interpreter/VM semantics, diagnostics, content hashes, and capability analysis in sync.
- Never commit credentials or edit generated files under `wiki/public/source/` by hand.
- Run the relevant tests while working and all release gates in the shared guide before claiming completion.
