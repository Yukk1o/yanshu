# VersionStore recovery and event integrity

Status: current v0.11 storage recovery contract.

Yanshu v0.11 keeps the file-backed version store, but no longer treats a sequence of independent file writes as a completed lifecycle operation. Registration, promotion, and rollback use a bounded recovery journal and an integrity-chained event log.

This design improves crash consistency without adding a database, native FFI, first-party `unsafe`, guest authority, or a new executable input format. `.yan` source and immutable metadata remain canonical.

## Transaction protocol

Every mutation holds `.yanshu-store.lock` and first completes any existing journal. A new mutation then follows this order:

```text
validate preconditions
        │
        ▼
atomically write + sync pending journal
        │
        ▼
write immutable source/metadata or active pointer
        │
        ▼
atomically replace + sync the complete event log
        │
        ▼
remove pending journal
```

The journal is operation-specific. It can describe only a registration or an active-pointer transition; recovery does not accept arbitrary paths. Source hashes, metadata shape, parent relationship, passing promotion report, event fields, lifecycle state, and hash-chain position are revalidated before replay.

Replay is idempotent:

- journal only: write the missing state and event;
- state written, event old: retain the state and append the planned event by atomic replacement;
- state and event written: verify both and remove the journal;
- conflicting state, malformed journal, or impossible ordering: fail closed with a stable diagnostic.

The implementation intentionally leaves a journal behind when a write or injected failure occurs. Removing it early would turn an observable error into permanent state/event divergence.

## Event format and legacy stores

New records use event schema v2. In addition to the lifecycle payload they contain:

- `schemaVersion: 2`;
- a one-based `sequence`;
- `previousHash`;
- `eventHash`, the SHA-256 of the canonical JSON record without `eventHash`.

Interior deletion, modification, duplication, and reordering fail structural or lifecycle validation. The event file is replaced atomically as a complete bounded document instead of being appended in place, so a partial final JSON line is not a normal recovery state.

Existing v0.10 event lines remain readable. The first v2 record after a legacy prefix uses the SHA-256 of the exact legacy bytes as `previousHash`; later records continue the v2 chain. This avoids rewriting historical files during upgrade.

The chain proves internal consistency, not authorship or completeness against an external checkpoint. An internally valid prefix can remain valid after tail truncation, and anyone able to rewrite the complete store can recompute unkeyed SHA-256 values. Key-managed signatures, an externally anchored head or remote transparency, and production approval identity remain separate future work.

## Explicit bounds

| Input | Limit |
| --- | ---: |
| immutable `.yan` source | 4 MiB |
| version metadata | 4 MiB |
| active pointer | 64 KiB |
| one event | 16 KiB |
| event log | 16 MiB / 65,536 events |
| recovery journal | 32 MiB |

File reads use a limited stream and reject non-regular files and observed symbolic links before allocation. The journal allows JSON escaping expansion of a maximum-size source while remaining bounded.

Every replaced file is `sync_all`'d before rename. Parent directories are also synchronized on Unix through safe standard-library APIs. Safe Rust's standard library does not expose the directory handle flags required for an equivalent Windows directory `fsync`; Windows therefore relies on synchronized files plus atomic rename and is not claimed to survive every filesystem/controller power-loss mode.

An interrupted atomic replacement may leave its uniquely named sibling temporary file behind before rename. Explicit recovery and every later mutation remove only names that exactly match Yanshu's internal target/process/sequence pattern while holding the store lock; unrelated files are preserved.

## Backup boundary

`backup-service` completes recovery before taking its long-lived version-store lease. Once held, no normal writer can create another journal during the snapshot. `verify-backup` is read-only: a recovery journal embedded in snapshot payload is rejected as an unexpected file and is never replayed.

Snapshot validation delegates event parsing to `yanshu-store`, then verifies that registered versions, active state, metadata parents, provider/timestamp fields, and passing promotion reports match the snapshot.

## Regression evidence

The store test suite injects failures after every durable stage of registration, promotion, and rollback, reopens the store, and checks exactly one event plus the intended final state. Separate tests cover stale temporary cleanup, legacy anchoring, and event modification/interior-deletion/reordering. The operations suite recomputes a snapshot checksum after corrupting `previousHash` and still requires semantic verification to reject it; it also proves that backup verification never replays an embedded journal.
