#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use yanshu_ops::{acquire_service_lease, create_backup, restore_backup, verify_backup};
use yanshu_store::{CandidateRegistration, VersionStore};

const SOURCE: &str =
    "(program (name backup-test) (version 1) (capabilities) (def main (fn () 1)) (export main))";

#[test]
fn backup_verifies_and_restores_without_overwriting() {
    let temporary = TestDirectory::new();
    let code = temporary.path.join("source-code");
    let data = temporary.path.join("source-data.json");
    let snapshot = temporary.path.join("snapshot");
    let restored_code = temporary.path.join("restored-code");
    let restored_data = temporary.path.join("restored-data.json");
    let hash = deployed_store(&code);
    fs::write(
        &data,
        b"{\"version\":1,\"entries\":[{\"key\":\"task/1\",\"value\":{\"title\":\"kept\"}}]}\n",
    )
    .unwrap_or_else(|error| panic!("fixture write failed: {error}"));

    let created =
        create_backup(&code, &data, &snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(created["activeVersion"], hash);
    assert!(created["files"].as_u64().is_some_and(|count| count >= 5));
    let verified = verify_backup(&snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(verified["activeVersion"], hash);

    restore_backup(&snapshot, &restored_code, &restored_data)
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let restored = VersionStore::new(&restored_code);
    assert_eq!(
        restored
            .active_source()
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}")),
        SOURCE
    );
    assert_eq!(read(&restored_data), read(&data));

    let diagnostic = restore_backup(&snapshot, &restored_code, &restored_data)
        .err()
        .unwrap_or_else(|| panic!("restore must refuse existing targets"));
    assert_eq!(diagnostic.code, "RESTORE_TARGET_EXISTS");
}

#[test]
fn tampering_and_concurrent_maintenance_fail_closed() {
    let temporary = TestDirectory::new();
    let code = temporary.path.join("code");
    let data = temporary.path.join("data.json");
    let snapshot = temporary.path.join("snapshot");
    let hash = deployed_store(&code);
    fs::write(&data, b"{\"version\":1,\"entries\":[]}\n")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));

    let lease = acquire_service_lease(&data).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let diagnostic = acquire_service_lease(&data)
        .err()
        .unwrap_or_else(|| panic!("second lease must fail"));
    assert_eq!(diagnostic.code, "SERVICE_MAINTENANCE_LOCKED");
    drop(lease);

    create_backup(&code, &data, &snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let source_path = snapshot
        .join("payload")
        .join("code")
        .join("versions")
        .join(format!("{hash}.yan"));
    fs::write(&source_path, b"tampered")
        .unwrap_or_else(|error| panic!("tamper write failed: {error}"));
    let diagnostic = verify_backup(&snapshot)
        .err()
        .unwrap_or_else(|| panic!("tampered snapshot must fail"));
    assert_eq!(diagnostic.code, "BACKUP_HASH_MISMATCH");
}

#[test]
fn a_missing_data_store_round_trips_as_missing() {
    let temporary = TestDirectory::new();
    let code = temporary.path.join("code");
    let data = temporary.path.join("never-created.json");
    let snapshot = temporary.path.join("snapshot");
    let restored_code = temporary.path.join("restored-code");
    let restored_data = temporary.path.join("restored-data.json");
    deployed_store(&code);

    let report =
        create_backup(&code, &data, &snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(report["dataStorePresent"], false);
    restore_backup(&snapshot, &restored_code, &restored_data)
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(!restored_data.exists());
    assert!(VersionStore::new(restored_code).active_hash().is_ok());
}

#[test]
fn event_chain_corruption_fails_after_snapshot_checksum_is_recomputed() {
    let temporary = TestDirectory::new();
    let code = temporary.path.join("code");
    let data = temporary.path.join("data.json");
    let snapshot = temporary.path.join("snapshot");
    deployed_store(&code);
    fs::write(&data, b"{\"version\":1,\"entries\":[]}\n")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    create_backup(&code, &data, &snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

    let events = snapshot.join("payload").join("code").join("events.jsonl");
    let source =
        fs::read_to_string(&events).unwrap_or_else(|error| panic!("event read failed: {error}"));
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let second = lines
        .get_mut(1)
        .unwrap_or_else(|| panic!("promoted event fixture must exist"));
    let mut event: JsonValue =
        serde_json::from_str(second).unwrap_or_else(|error| panic!("event parse failed: {error}"));
    event["previousHash"] = json!("0".repeat(64));
    *second = event.to_string();
    let corrupted = format!("{}\n", lines.join("\n")).into_bytes();
    fs::write(&events, &corrupted)
        .unwrap_or_else(|error| panic!("event corruption failed: {error}"));

    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: JsonValue = serde_json::from_slice(&read(&manifest_path))
        .unwrap_or_else(|error| panic!("manifest parse failed: {error}"));
    let entries = manifest["files"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("manifest files must be an array"));
    let entry = entries
        .iter_mut()
        .find(|entry| entry["path"] == "code/events.jsonl")
        .unwrap_or_else(|| panic!("event manifest entry must exist"));
    entry["bytes"] = json!(corrupted.len());
    entry["sha256"] = json!(hex_digest(&corrupted));
    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest)
            .unwrap_or_else(|error| panic!("manifest encode failed: {error}")),
    )
    .unwrap_or_else(|error| panic!("manifest rewrite failed: {error}"));

    let diagnostic = verify_backup(&snapshot)
        .err()
        .unwrap_or_else(|| panic!("event chain corruption must fail"));
    assert_eq!(diagnostic.code, "BACKUP_INVALID_EVENTS");
}

#[test]
fn backup_verification_never_replays_an_embedded_recovery_journal() {
    let temporary = TestDirectory::new();
    let code = temporary.path.join("code");
    let data = temporary.path.join("data.json");
    let snapshot = temporary.path.join("snapshot");
    deployed_store(&code);
    fs::write(&data, b"{\"version\":1,\"entries\":[]}\n")
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    create_backup(&code, &data, &snapshot).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

    let journal = snapshot
        .join("payload")
        .join("code")
        .join(".yanshu-store.pending.json");
    let journal_bytes = b"{}\n";
    fs::write(&journal, journal_bytes)
        .unwrap_or_else(|error| panic!("journal fixture failed: {error}"));
    let manifest_path = snapshot.join("manifest.json");
    let mut manifest: JsonValue = serde_json::from_slice(&read(&manifest_path))
        .unwrap_or_else(|error| panic!("manifest parse failed: {error}"));
    manifest["files"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("manifest files must be an array"))
        .push(json!({
            "path": "code/.yanshu-store.pending.json",
            "bytes": journal_bytes.len(),
            "sha256": hex_digest(journal_bytes),
        }));
    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest)
            .unwrap_or_else(|error| panic!("manifest encode failed: {error}")),
    )
    .unwrap_or_else(|error| panic!("manifest rewrite failed: {error}"));

    let diagnostic = verify_backup(&snapshot)
        .err()
        .unwrap_or_else(|| panic!("embedded recovery journal must be rejected"));
    assert_eq!(diagnostic.code, "BACKUP_UNEXPECTED_STORE_FILE");
    assert_eq!(read(&journal), journal_bytes);
}

fn deployed_store(path: &Path) -> String {
    let store = VersionStore::new(path);
    let report = json!({ "passed": true });
    let metadata = json!({});
    let hash = store
        .register_candidate(CandidateRegistration {
            source: SOURCE,
            parent: None,
            provider: "backup-test",
            provider_metadata: &metadata,
            report: &report,
            registered_at: 1,
        })
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    store
        .promote(&hash, 2)
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    hash
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("fixture read failed: {error}"))
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
            "ai-lang-rust-ops-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
