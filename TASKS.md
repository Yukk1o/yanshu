# Implementation Tasks

- [x] Define v0.1 scope, semantics, trust boundary, and acceptance scenario.
- [x] Implement structured errors, reader, AST, and program parser.
- [x] Implement the bounded evaluator and pure primitive library.
- [x] Implement JSON test suites and reports.
- [x] Implement content-addressed versions, promotion, and rollback.
- [x] Implement provider boundary and deterministic offline provider.
- [x] Implement CLI, example, and end-to-end demonstration.
- [x] Add conformance tests and verify the complete workflow.

## Change: add-live-llm-provider

- [x] Define the live provider configuration, prompt, response schema, and safety boundary.
- [x] Implement bounded HTTPS JSON transport.
- [x] Implement an OpenAI Responses API provider with Structured Outputs.
- [x] Record provider model and response metadata in candidate versions.
- [x] Add a one-step live evolution CLI with explicit promotion.
- [x] Test request construction, output extraction, refusals, and regression behavior.
- [x] Add a DeepSeek V4 Chat Completions adapter with JSON Output validation.
- [x] Run a live smoke test from a caller-configured process environment; chat secrets are never copied into command logs.

## Change: web-backend-runtime

- [x] Add explicit static route declarations and request/response contracts.
- [x] Inject `kv`, `clock`, and `log` capabilities without ambient authority.
- [x] Add transactional in-memory and atomically persisted JSON KV adapters.
- [x] Add a bounded concurrent HTTP/1.1 JSON host and redacted observations.
- [x] Build a complete task CRUD service and stateful business scenario suite.
- [x] Gate deployment and LLM candidates on the service scenario suite.
- [x] Pin the active program once per request and support explicit rollback.
- [x] Add a responsive same-origin browser console for end-to-end testing.
- [x] Verify malformed input, rollback, concurrency, restart persistence, and hot version selection.

## Change: business-backend-v0.3

- [x] Define a bounded compiler-owned Schema grammar.
- [x] Parse schemas into typed AST nodes and expose them through inspection.
- [x] Return normalized `Ok` values or bounded structured `Err` issues.
- [x] Charge recursive validation work against interpreter fuel.
- [x] Add uniform `api-response` and `api-error` constructors.
- [x] Migrate task create/update handlers away from manual type branches.
- [x] Expand the stateful business suite from 8 to 11 scenarios.
- [x] Teach OpenAI and DeepSeek providers the schema and API contracts.
- [x] Verify the v0.3 envelope over real HTTP.

## Change: library-backend-v0.4

- [x] Define versioned library contracts separately from capabilities and implementations.
- [x] Parse bounded `libraries` declarations into inspectable AST metadata.
- [x] Add a contract-owned function set, type boundary, and fuel estimator.
- [x] Add exact-version backend registration with strict implementation matching.
- [x] Normalize and bound backend results and redact unexpected backend failures.
- [x] Ship and test a `text@1` Racket reference backend.
- [x] Thread backend selection through pure, service, and HTTP execution paths.
- [x] Teach live providers the library declaration and comment policy.
- [x] Verify the complete repository and document the v0.4 checkpoint.

## Change: rust-host-v0.5

- [x] Freeze a host-neutral v1 conformance manifest and portable value codec.
- [x] Make the Racket oracle execute the manifest through the public CLI.
- [x] Establish a Rust workspace whose first-party crates forbid unsafe code.
- [x] Implement Rust diagnostics, Reader, source spans, AST, and program parser.
- [x] Implement the bounded Rust evaluator and `text@1` Library Backend.
- [x] Run Racket and Rust against the same canonical fixtures with zero differences.
- [ ] Migrate schema, service, store, HTTP, version, and provider layers in stages.
- [ ] Pass dependency audit, shadow-traffic, rollback, observability, and release gates.
