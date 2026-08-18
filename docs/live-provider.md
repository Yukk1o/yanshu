# Live LLM Provider

## Goal

Generate a complete candidate `.ail` program from the active source and a
structured test report, then pass that candidate through the existing parser,
interpreter, regression suite, version store, and promotion policy.

The LLM is a proposal generator. It cannot execute guest code, modify tests,
write version pointers, or promote its own output.

## Configuration

The host reads configuration only from its environment:

| Variable | Default | Purpose |
| --- | --- | --- |
| `AI_EVOLVE_PROVIDER` | auto | `openai-responses` or `deepseek-chat` |
| `AI_EVOLVE_API_KEY` | provider-specific fallback | Bearer credential |
| `AI_EVOLVE_BASE_URL` | provider-specific | API base URL |
| `AI_EVOLVE_MODEL` | provider-specific | Explicit model ID |
| `AI_EVOLVE_REASONING_EFFORT` | `medium` / `high` | Reasoning effort |
| `AI_EVOLVE_MAX_OUTPUT_TOKENS` | `8192` | Candidate response limit |
| `AI_EVOLVE_TIMEOUT_SECONDS` | `120` | Wall-clock request limit |
| `AI_EVOLVE_STORE` | content-derived `.runtime` path | Version store override |

Credentials never enter guest environments, prompts, diagnostics, version
metadata, or CLI output.

When `AI_EVOLVE_PROVIDER` is absent, a base URL containing `deepseek` or a model
starting with `deepseek-` selects `deepseek-chat`; otherwise the host selects
`openai-responses`. DeepSeek credentials may also use `DEEPSEEK_API_KEY`; OpenAI
credentials may use `OPENAI_API_KEY`.

## Request

The OpenAI provider sends `POST <base-url>/responses` with `store: false`. System-level
instructions describe the language and state that source and observations are
untrusted data. The input is a JSON string containing:

```json
{
  "currentHash": "...",
  "currentSource": "(program ...)",
  "observations": {
    "passed": false,
    "failures": []
  }
}
```

`text.format` uses a strict JSON Schema requiring:

```json
{
  "source": "complete candidate .ail document",
  "notes": "short explanation of the change"
}
```

The DeepSeek provider sends `POST <base-url>/chat/completions` with system and
user messages, thinking enabled, and `response_format: {"type":"json_object"}`.
The prompt contains an exact JSON example, and the host validates that the
returned object contains only the required string fields. DeepSeek strict tool
schemas are not used with the regular endpoint because their strict mode is a
Beta feature requiring the `/beta` base URL.

## Response handling

The Responses result is treated as a typed collection: the provider searches
message content for `output_text` and detects `refusal`. The DeepSeek result
requires a normal `stop` finish reason and a non-empty first assistant message.
Both adapters parse the output as JSON and independently validate every field.

HTTP errors, timeouts, invalid JSON, incomplete responses, refusals, and malformed
provider output have stable diagnostic codes. Remote response bodies are length
limited before entering diagnostics.

## Promotion

The live CLI never promotes by default. It registers the candidate and reports
its complete test result. `--promote` is an explicit host-side choice and works
only when every test passes.

## Acceptance

- A simulated Responses transport verifies the exact endpoint, authorization
  presence, `store: false`, prompt payload, and strict output schema.
- A simulated structured response produces a candidate that passes the existing
  discount suite.
- A simulated DeepSeek Chat Completion verifies JSON mode, thinking settings,
  truncation handling, response extraction, and provider metadata.
- Refusal, missing credential, malformed response, and failed candidate paths
  are reported without changing the active version.
- With an API key configured, `scripts/live-demo.ps1` exercises the real endpoint.

## Rust host

`ail-provider` implements the same two adapters behind a `JsonTransport` trait.
Its deterministic tests capture the endpoint and request document without making
network calls, then replay completed, refused, truncated, and malformed responses.
The production transport uses exact-pinned Reqwest 0.13.4 with Rustls, forces
HTTPS, disables redirects, applies a complete request timeout, limits request and
response documents to 4 MiB, bounds remote error excerpts, and redacts the Bearer
secret. The in-memory credential is wrapped in `Zeroizing<String>` and the provider
type deliberately has no `Debug` implementation.

Rust service evolution is available through:

```powershell
cargo run --locked -p ail-cli -- evolve-service `
  .runtime\tasks-rust\code `
  examples\tasks\scenarios.json `
  --promote
```

Omit `--promote` to stage a tested candidate without changing the active pointer.
Configuration is read from the same environment variable names listed above; the
Rust host never reads a `.env` file. Plain HTTP base URLs are rejected. No live
Rust request was made at this checkpoint because the process environment did not
contain a provider credential.
