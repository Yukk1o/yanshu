#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::YanshuResult;

use super::{CandidateRegistration, VersionStore, source_hash};

const INITIAL_SOURCE: &str = include_str!("../../../../examples/discount/v1.yan");
const CANDIDATE_SOURCE: &str = include_str!("../../../../examples/discount/v2.yan");
const BUSINESS_V2_SOURCE: &str = include_str!("../../../../examples/expenses/service.yan");
const INITIAL_HASH: &str = "2f16c05a312992b3e424b57743fe6283023901dabb3a1a2e57b9cc8e75726329";
const CANDIDATE_HASH: &str = "1c238ff3c4ae7bc292f06801a114f9029de243ff3d0a1aeaf808d7a483bd97b4";

fn require<T>(result: YanshuResult<T>) -> T {
    match result {
        Ok(value) => value,
        Err(diagnostic) => panic!("{diagnostic}"),
    }
}

fn registration<'value>(
    source: &'value str,
    parent: Option<&'value str>,
    report: &'value JsonValue,
    at: u64,
) -> CandidateRegistration<'value> {
    CandidateRegistration {
        source,
        parent,
        provider: "rust-test",
        provider_metadata: &JsonValue::Null,
        report,
        registered_at: at,
    }
}

fn error_code<T>(result: YanshuResult<T>) -> &'static str {
    match result {
        Ok(_) => panic!("operation unexpectedly succeeded"),
        Err(diagnostic) => diagnostic.code,
    }
}

#[test]
fn hashes_match_sha256_and_canonical_version_ids() {
    assert_eq!(
        source_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(source_hash(INITIAL_SOURCE), INITIAL_HASH);
    assert_eq!(source_hash(CANDIDATE_SOURCE), CANDIDATE_HASH);
}

#[test]
fn stores_language_version_separately_from_content_identity() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    let passing = json!({ "passed": true });
    let first =
        require(store.register_candidate(registration(BUSINESS_V2_SOURCE, None, &passing, 1)));
    let changed_source = BUSINESS_V2_SOURCE.replace(
        "(name expense-approval)",
        "(name expense-approval-candidate)",
    );
    let second =
        require(store.register_candidate(registration(&changed_source, None, &passing, 2)));

    assert_ne!(first, second);
    assert_eq!(
        require(store.version_metadata(&first)).get("languageVersion"),
        Some(&json!(2))
    );
    assert_eq!(
        require(store.version_metadata(&second)).get("languageVersion"),
        Some(&json!(2))
    );
    assert_eq!(first, source_hash(BUSINESS_V2_SOURCE));
    assert_eq!(second, source_hash(&changed_source));
}

#[test]
fn repeated_content_cannot_claim_conflicting_immutable_metadata() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    let passing = json!({ "passed": true });
    let hash = require(store.register_candidate(registration(CANDIDATE_SOURCE, None, &passing, 1)));
    assert_eq!(
        error_code(store.register_candidate(registration(
            CANDIDATE_SOURCE,
            Some(INITIAL_HASH),
            &passing,
            1,
        ))),
        "VERSION_REGISTRATION_CONFLICT"
    );
    assert_eq!(
        require(store.version_metadata(&hash))["parent"],
        JsonValue::Null
    );
}

#[test]
fn repeated_identical_registration_preserves_the_first_timestamp_and_event() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    let passing = json!({ "passed": true });
    let first =
        require(store.register_candidate(registration(CANDIDATE_SOURCE, None, &passing, 10)));
    let repeated =
        require(store.register_candidate(registration(CANDIDATE_SOURCE, None, &passing, 20)));
    assert_eq!(repeated, first);
    assert_eq!(require(store.version_metadata(&first))["registeredAt"], 10);
    assert_eq!(read_events(&temporary).lines().count(), 1);
}

#[test]
fn promotes_reopens_and_rolls_back_immutable_versions() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    let passing = json!({ "passed": true });
    let initial = require(store.register_candidate(registration(
        INITIAL_SOURCE,
        None,
        &passing,
        1_700_000_000,
    )));
    assert_eq!(initial, INITIAL_HASH);
    assert_eq!(require(store.promote(&initial, 1_700_000_001)), initial);
    let candidate = require(store.register_candidate(registration(
        CANDIDATE_SOURCE,
        Some(&initial),
        &passing,
        1_700_000_002,
    )));
    assert_eq!(candidate, CANDIDATE_HASH);
    assert_eq!(require(store.promote(&candidate, 1_700_000_003)), candidate);
    assert_eq!(require(store.active_hash()), Some(candidate.clone()));
    assert_eq!(require(store.active_source()), CANDIDATE_SOURCE);

    let reopened = VersionStore::new(&temporary.path);
    assert_eq!(require(reopened.active_hash()), Some(candidate));
    assert_eq!(require(reopened.rollback(1_700_000_004)), initial);
    assert_eq!(require(reopened.active_source()), INITIAL_SOURCE);
    assert_eq!(read_events(&temporary).lines().count(), 5);

    for (index, line) in read_events(&temporary).lines().enumerate() {
        let event: JsonValue =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("event parse: {error}"));
        assert_eq!(event["schemaVersion"], 2);
        assert_eq!(event["sequence"], index + 1);
        assert!(
            event["eventHash"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64)
        );
    }
}

#[test]
fn failed_reports_and_parent_mismatches_cannot_be_promoted() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    let failed = json!({ "passed": false });
    let hash = require(store.register_candidate(registration(INITIAL_SOURCE, None, &failed, 1)));
    assert_eq!(
        error_code(store.promote(&hash, 2)),
        "VERSION_TESTS_NOT_PASSED"
    );

    let passing = json!({ "passed": true });
    let candidate = require(store.register_candidate(registration(
        CANDIDATE_SOURCE,
        Some(INITIAL_HASH),
        &passing,
        3,
    )));
    assert_eq!(
        error_code(store.promote(&candidate, 4)),
        "VERSION_PARENT_MISMATCH"
    );
}

#[test]
fn rejects_path_traversal_and_detects_source_tampering() {
    let temporary = TestDirectory::new();
    let store = VersionStore::new(&temporary.path);
    assert_eq!(
        error_code(store.version_source("../active.json")),
        "VERSION_INVALID_HASH"
    );

    let passing = json!({ "passed": true });
    let hash = require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
    fs::write(
        temporary.path.join("versions").join(format!("{hash}.yan")),
        CANDIDATE_SOURCE,
    )
    .unwrap_or_else(|error| panic!("tamper fixture failed: {error}"));
    assert_eq!(
        error_code(store.version_source(&hash)),
        "VERSION_INTEGRITY_FAILURE"
    );
}

#[test]
fn cross_process_lock_fails_with_a_bounded_timeout() {
    let temporary = TestDirectory::new();
    let lock_path = temporary.path.join(".yanshu-store.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap_or_else(|error| panic!("lock fixture failed: {error}"));
    lock.lock()
        .unwrap_or_else(|error| panic!("lock acquisition failed: {error}"));
    let store = VersionStore::with_lock_timeout(&temporary.path, Duration::from_millis(20));
    let passing = json!({ "passed": true });

    assert_eq!(
        error_code(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1,))),
        "VERSION_LOCK_TIMEOUT"
    );
}

#[test]
fn recovery_and_event_files_are_bounded_before_parsing() {
    let journal_store = TestDirectory::new();
    let journal = fs::File::create(journal_store.path.join(".yanshu-store.pending.json"))
        .unwrap_or_else(|error| panic!("journal fixture failed: {error}"));
    journal
        .set_len(super::storage::MAXIMUM_JOURNAL_BYTES + 1)
        .unwrap_or_else(|error| panic!("journal limit fixture failed: {error}"));
    assert_eq!(
        error_code(VersionStore::new(&journal_store.path).active_hash()),
        "VERSION_RECOVERY_INVALID"
    );

    let event_store = TestDirectory::new();
    let events = fs::File::create(event_store.path.join("events.jsonl"))
        .unwrap_or_else(|error| panic!("event fixture failed: {error}"));
    events
        .set_len(super::storage::MAXIMUM_EVENT_LOG_BYTES + 1)
        .unwrap_or_else(|error| panic!("event limit fixture failed: {error}"));
    let passing = json!({ "passed": true });
    assert_eq!(
        error_code(
            VersionStore::new(&event_store.path).register_candidate(registration(
                INITIAL_SOURCE,
                None,
                &passing,
                1,
            ))
        ),
        "VERSION_INVALID_STORE"
    );
}

#[test]
fn explicit_recovery_removes_only_recognized_stale_temporary_files() {
    let temporary = TestDirectory::new();
    let versions = temporary.path.join("versions");
    let metadata = temporary.path.join("metadata");
    fs::create_dir_all(&versions)
        .and_then(|()| fs::create_dir_all(&metadata))
        .unwrap_or_else(|error| panic!("temporary directory fixture failed: {error}"));
    let stale_root = temporary.path.join("..yanshu-store.pending.json.42.1.tmp");
    let stale_source = versions.join(format!(".{INITIAL_HASH}.yan.42.2.tmp"));
    let stale_metadata = metadata.join(format!(".{INITIAL_HASH}.json.42.3.tmp"));
    let unrelated = temporary.path.join(".notes.42.4.tmp");
    for path in [&stale_root, &stale_source, &stale_metadata, &unrelated] {
        fs::write(path, b"stale").unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    }

    require(VersionStore::new(&temporary.path).recover());

    assert!(!stale_root.exists());
    assert!(!stale_source.exists());
    assert!(!stale_metadata.exists());
    assert!(unrelated.exists());
}

#[test]
fn registration_recovers_idempotently_after_every_durable_step() {
    for step in 1..=4 {
        let temporary = TestDirectory::new();
        let passing = json!({ "passed": true });
        let failing = VersionStore::with_failure_after_step(&temporary.path, step);
        assert_eq!(
            error_code(failing.register_candidate(registration(
                INITIAL_SOURCE,
                None,
                &passing,
                10,
            ))),
            "VERSION_INJECTED_FAILURE"
        );

        let reopened = VersionStore::new(&temporary.path);
        assert_eq!(
            require(reopened.version_source(INITIAL_HASH)),
            INITIAL_SOURCE
        );
        assert_eq!(
            require(reopened.version_metadata(INITIAL_HASH))["registeredAt"],
            10
        );
        assert_eq!(read_events(&temporary).lines().count(), 1);
        assert!(!temporary.path.join(".yanshu-store.pending.json").exists());
    }
}

#[test]
fn promotion_recovers_idempotently_after_every_durable_step() {
    for step in 1..=3 {
        let temporary = TestDirectory::new();
        let passing = json!({ "passed": true });
        let store = VersionStore::new(&temporary.path);
        let initial =
            require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
        let failing = VersionStore::with_failure_after_step(&temporary.path, step);
        assert_eq!(
            error_code(failing.promote(&initial, 2)),
            "VERSION_INJECTED_FAILURE"
        );

        let reopened = VersionStore::new(&temporary.path);
        assert_eq!(require(reopened.active_hash()), Some(initial));
        assert_eq!(read_events(&temporary).lines().count(), 2);
        assert!(!temporary.path.join(".yanshu-store.pending.json").exists());
    }
}

#[test]
fn rollback_recovers_idempotently_after_every_durable_step() {
    for step in 1..=3 {
        let temporary = TestDirectory::new();
        let passing = json!({ "passed": true });
        let store = VersionStore::new(&temporary.path);
        let initial =
            require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
        require(store.promote(&initial, 2));
        let candidate = require(store.register_candidate(registration(
            CANDIDATE_SOURCE,
            Some(&initial),
            &passing,
            3,
        )));
        require(store.promote(&candidate, 4));

        let failing = VersionStore::with_failure_after_step(&temporary.path, step);
        assert_eq!(error_code(failing.rollback(5)), "VERSION_INJECTED_FAILURE");
        let reopened = VersionStore::new(&temporary.path);
        assert_eq!(require(reopened.active_hash()), Some(initial));
        assert_eq!(read_events(&temporary).lines().count(), 5);
        assert!(!temporary.path.join(".yanshu-store.pending.json").exists());
    }
}

#[test]
fn event_chain_rejects_modification_interior_deletion_and_reordering() {
    let temporary = TestDirectory::new();
    let store = deployed_two_version_store(&temporary);
    let original = read_events(&temporary);
    let lines = original.lines().collect::<Vec<_>>();
    let versions = BTreeSet::from([INITIAL_HASH.to_owned(), CANDIDATE_HASH.to_owned()]);

    let mut modified: JsonValue = serde_json::from_str(lines[0])
        .unwrap_or_else(|error| panic!("event parse failed: {error}"));
    modified["at"] = json!(999);
    let modified_log = format!("{}\n{}\n{}\n{}\n", modified, lines[1], lines[2], lines[3]);
    assert_eq!(
        error_code(store.validate_event_log(
            modified_log.as_bytes(),
            &versions,
            Some(CANDIDATE_HASH),
        )),
        "VERSION_INVALID_EVENTS"
    );

    let deleted = format!("{}\n{}\n{}\n", lines[0], lines[2], lines[3]);
    assert_eq!(
        error_code(store.validate_event_log(deleted.as_bytes(), &versions, Some(CANDIDATE_HASH),)),
        "VERSION_INVALID_EVENTS"
    );

    let reordered = format!("{}\n{}\n{}\n{}\n", lines[1], lines[0], lines[2], lines[3]);
    assert_eq!(
        error_code(
            store.validate_event_log(reordered.as_bytes(), &versions, Some(CANDIDATE_HASH),)
        ),
        "VERSION_INVALID_EVENTS"
    );
}

#[test]
fn first_chained_event_anchors_an_existing_legacy_log() {
    let temporary = TestDirectory::new();
    let passing = json!({ "passed": true });
    let store = VersionStore::new(&temporary.path);
    let initial =
        require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
    require(store.promote(&initial, 2));

    let mut legacy = Vec::new();
    for line in read_events(&temporary).lines() {
        let mut event: JsonValue = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("event parse failed: {error}"));
        let object = event
            .as_object_mut()
            .unwrap_or_else(|| panic!("event must be an object"));
        for field in ["schemaVersion", "sequence", "previousHash", "eventHash"] {
            object.remove(field);
        }
        legacy.extend_from_slice(event.to_string().as_bytes());
        legacy.push(b'\n');
    }
    fs::write(temporary.path.join("events.jsonl"), &legacy)
        .unwrap_or_else(|error| panic!("legacy fixture failed: {error}"));

    require(store.register_candidate(registration(CANDIDATE_SOURCE, Some(&initial), &passing, 3)));
    let events = read_events(&temporary);
    let third: JsonValue = serde_json::from_str(
        events
            .lines()
            .nth(2)
            .unwrap_or_else(|| panic!("third event must exist")),
    )
    .unwrap_or_else(|error| panic!("event parse failed: {error}"));
    assert_eq!(third["sequence"], 3);
    assert_eq!(third["previousHash"], super::storage::sha256_hex(&legacy));
}

fn deployed_two_version_store(temporary: &TestDirectory) -> VersionStore {
    let passing = json!({ "passed": true });
    let store = VersionStore::new(&temporary.path);
    let initial =
        require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
    require(store.promote(&initial, 2));
    let candidate = require(store.register_candidate(registration(
        CANDIDATE_SOURCE,
        Some(&initial),
        &passing,
        3,
    )));
    require(store.promote(&candidate, 4));
    store
}

fn read_events(temporary: &TestDirectory) -> String {
    fs::read_to_string(temporary.path.join("events.jsonl"))
        .unwrap_or_else(|error| panic!("events read failed: {error}"))
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yanshu-rust-store-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
