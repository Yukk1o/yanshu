# Web Backend Specification v0.2

## Objective

Run an Yanshu program as a real JSON HTTP service while preserving the core
trust boundary: guest source defines business policy; the host owns sockets,
resource limits, persistence, version selection, and promotion.

## Service declarations

A program may declare static routes:

```lisp
(program
  (name tasks)
  (version 1)
  (capabilities kv log)
  (route GET    "/tasks"      list-tasks)
  (route POST   "/tasks"      create-task)
  (route GET    "/tasks/:id"  get-task)
  (route PUT    "/tasks/:id"  update-task)
  (route DELETE "/tasks/:id"  delete-task)
  (def ...)
  (export list-tasks create-task get-task update-task delete-task))
```

Methods are `GET`, `POST`, `PUT`, `PATCH`, and `DELETE`. A path segment beginning
with `:` captures one decoded parameter. Routes must be unique and every handler
must be exported. Route matching order is declaration order, with duplicate or
ambiguous patterns rejected at parse time.

## Handler contract

Every route handler receives one immutable request map:

```json
{
  "method": "GET",
  "path": "/tasks/42",
  "params": {"id": "42"},
  "query": {"limit": "10"},
  "headers": {"content-type": "application/json"},
  "body": null
}
```

It returns a response map containing exactly:

```json
{
  "status": 200,
  "headers": {"content-type": "application/json"},
  "body": {"id": "42", "title": "ship v0.2"}
}
```

The host validates status, headers, and JSON serializability before writing any
bytes. A malformed response becomes a structured 500 response and is recorded
as an observation.

## Capabilities

Pure programs retain no ambient authority. Web programs can request:

- `log`: structured application logging.
- `kv`: transactional JSON-value storage through `kv-get`, `kv-put`,
  `kv-delete`, and `kv-list`.
- `clock`: UTC Unix milliseconds through `now-ms`.

Capability implementations are injected by the host. They cannot expose Racket
objects or arbitrary callbacks to guest code.

Each request uses one KV transaction. Mutations commit only after the handler
finishes and the response contract validates. Runtime errors, timeouts, and
invalid responses discard the transaction. The first adapter is in-memory for
tests; the runnable demo uses an atomically replaced JSON file so restarts retain
business data. The interface remains replaceable by a Rust/PostgreSQL adapter.

## HTTP host

The prototype HTTP/1.1 host supports non-streaming JSON requests and responses:

- one request per connection with `Connection: close`;
- bounded request line, headers, and body;
- configurable wall-clock timeout, interpreter fuel, and call depth;
- lowercase normalized header names;
- UTF-8 JSON bodies and structured errors;
- concurrent connections with a fixed worker limit;
- graceful shutdown for tests and local development.

The server chooses the active source hash once at request start. Promotion can
affect later requests but never changes the program executing an in-flight
request.

## Failure mapping

- malformed HTTP or JSON: `400`;
- no matching route: `404`;
- unsupported method/media type: `405` or `415`;
- guest diagnostic or invalid response: `500` with a public request ID;
- exhausted wall-clock budget: `504`.

Internal diagnostics go to observations and logs, not to public response bodies.

## Evolution boundary

The LLM may receive source plus redacted aggregate observations: route, status,
diagnostic code, test failure, and latency bucket. It never receives credentials,
authorization headers, raw personal data, or the persistence file. A candidate
must pass language tests, direct handler tests, and real HTTP contract tests.
Production promotion remains an explicit host operation.

## Acceptance service

`examples/tasks/` proves the backend surface with create, list, read, update,
and delete operations. Acceptance requires:

1. real TCP requests exercise all five routes;
2. JSON data survives a server restart with the file adapter;
3. malformed input and missing resources return stable statuses;
4. a failing handler cannot commit KV writes;
5. concurrent requests do not corrupt the store;
6. an active version remains pinned for each request;
7. all existing v0.1 language and evolution tests continue to pass.

## Implemented checkpoint

The v0.2 prototype implements every acceptance item above on
`feature/web-backend-runtime`. `scripts/serve-tasks.ps1` runs the stateful suite,
promotes the content-addressed service version, then serves both the JSON API and
the same-origin browser console on loopback. `deploy-service`, `serve-active`,
`evolve-service`, and `rollback-service` expose the same lifecycle through the
JSON CLI.

This checkpoint is intentionally a local backend runtime. Public deployment still
requires an authenticated reverse proxy, TLS, a production database adapter,
process isolation, migrations/backups, and operational rollout policy.
