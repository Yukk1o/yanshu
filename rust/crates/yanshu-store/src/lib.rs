#![forbid(unsafe_code)]

mod scenario;

pub use scenario::run_version_scenario;

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::{Program, load_program_source};

#[derive(Debug, Clone, Copy)]
pub struct CandidateRegistration<'value> {
    pub source: &'value str,
    pub parent: Option<&'value str>,
    pub provider: &'value str,
    pub provider_metadata: &'value JsonValue,
    pub report: &'value JsonValue,
    pub registered_at: u64,
}

#[derive(Debug)]
pub struct VersionStore {
    root: PathBuf,
    lock_timeout: Duration,
}

impl VersionStore {
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            lock_timeout: Duration::from_secs(5),
        }
    }

    #[must_use]
    pub fn with_lock_timeout(root: impl AsRef<Path>, lock_timeout: Duration) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            lock_timeout,
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn register_candidate(
        &self,
        registration: CandidateRegistration<'_>,
    ) -> YanshuResult<String> {
        let program = load_program_source(registration.source)?;
        if let Some(parent) = registration.parent {
            validate_hash(parent)?;
        }
        self.with_write_lock(|| self.register_candidate_unlocked(registration, &program))
    }

    pub fn promote(&self, hash: &str, at: u64) -> YanshuResult<String> {
        validate_hash(hash)?;
        self.with_write_lock(|| self.promote_unlocked(hash, at))
    }

    pub fn rollback(&self, at: u64) -> YanshuResult<String> {
        self.with_write_lock(|| self.rollback_unlocked(at))
    }

    pub fn active_hash(&self) -> YanshuResult<Option<String>> {
        self.active_hash_unlocked()
    }

    pub fn active_source(&self) -> YanshuResult<String> {
        let hash = self.active_hash_unlocked()?.ok_or_else(|| {
            Diagnostic::simple("VERSION_NO_ACTIVE", "version store has no active version")
        })?;
        self.version_source_unlocked(&hash)
    }

    pub fn version_source(&self, hash: &str) -> YanshuResult<String> {
        validate_hash(hash)?;
        self.version_source_unlocked(hash)
    }

    pub fn version_metadata(&self, hash: &str) -> YanshuResult<JsonValue> {
        validate_hash(hash)?;
        self.version_metadata_unlocked(hash)
    }

    fn with_write_lock<T>(&self, operation: impl FnOnce() -> YanshuResult<T>) -> YanshuResult<T> {
        fs::create_dir_all(&self.root).map_err(|_| lock_failure(&self.root))?;
        let lock_path = self.root.join(".yanshu-store.lock");
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| lock_failure(&lock_path))?;
        let started = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if started.elapsed() < self.lock_timeout => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(Diagnostic::new(
                        "VERSION_LOCK_TIMEOUT",
                        "version store write lock timed out",
                        json!({
                            "root": self.root.display().to_string(),
                            "timeoutMs": self.lock_timeout.as_millis().to_string(),
                        }),
                    ));
                }
                Err(fs::TryLockError::Error(_)) => return Err(lock_failure(&lock_path)),
            }
        }
        operation()
    }

    fn register_candidate_unlocked(
        &self,
        registration: CandidateRegistration<'_>,
        program: &Program,
    ) -> YanshuResult<String> {
        let hash = source_hash(registration.source);
        self.ensure_directories()?;
        let source_path = self.version_source_path(&hash);
        if source_path.exists() {
            let existing = self.version_source_unlocked(&hash)?;
            if existing != registration.source {
                return Err(integrity_failure(&hash));
            }
        } else {
            atomic_replace(&source_path, registration.source.as_bytes())
                .map_err(|_| write_failure(&source_path))?;
        }

        let language_version: JsonValue = serde_json::from_str(&program.version.to_string())
            .map_err(|_| {
                Diagnostic::simple(
                    "VERSION_INVALID_PROGRAM",
                    "program language version cannot be represented as JSON",
                )
            })?;
        let parent = registration
            .parent
            .map_or(JsonValue::Null, |value| JsonValue::String(value.to_owned()));
        let mut metadata = json!({
            "hash": hash,
            "parent": parent,
            "program": program.name,
            "languageVersion": language_version,
            "provider": registration.provider,
            "providerMetadata": registration.provider_metadata,
            "registeredAt": registration.registered_at,
            "report": registration.report,
        });
        let metadata_path = self.version_metadata_path(&hash);
        if metadata_path.exists() {
            let existing = self.version_metadata_unlocked(&hash)?;
            metadata["registeredAt"] = existing["registeredAt"].clone();
            if existing != metadata {
                return Err(Diagnostic::new(
                    "VERSION_REGISTRATION_CONFLICT",
                    "content-addressed source already has different immutable metadata",
                    json!({ "hash": hash }),
                ));
            }
            return Ok(hash);
        } else {
            write_json_atomically(&metadata_path, &metadata)?;
        }

        self.append_event(&json!({
            "event": "registered",
            "hash": hash,
            "parent": registration.parent,
            "provider": registration.provider,
            "at": registration.registered_at,
        }))?;
        Ok(hash)
    }

    fn promote_unlocked(&self, hash: &str, at: u64) -> YanshuResult<String> {
        let metadata = self.version_metadata_unlocked(hash)?;
        self.version_source_unlocked(hash)?;
        if metadata
            .get("report")
            .and_then(JsonValue::as_object)
            .and_then(|report| report.get("passed"))
            .and_then(JsonValue::as_bool)
            != Some(true)
        {
            return Err(Diagnostic::new(
                "VERSION_TESTS_NOT_PASSED",
                "candidate cannot be promoted before its test report passes",
                json!({ "hash": hash }),
            ));
        }
        let current = self.active_hash_unlocked()?;
        let parent = metadata_parent(&metadata)?;
        if parent != current {
            return Err(Diagnostic::new(
                "VERSION_PARENT_MISMATCH",
                "candidate parent is not the active version",
                json!({
                    "hash": hash,
                    "candidateParent": parent,
                    "active": current,
                }),
            ));
        }
        self.write_active_pointer(hash)?;
        self.append_event(&json!({
            "event": "promoted",
            "from": current,
            "to": hash,
            "at": at,
        }))?;
        Ok(hash.to_owned())
    }

    fn rollback_unlocked(&self, at: u64) -> YanshuResult<String> {
        let current = self.active_hash_unlocked()?.ok_or_else(|| {
            Diagnostic::simple("VERSION_NO_ACTIVE", "version store has no active version")
        })?;
        let metadata = self.version_metadata_unlocked(&current)?;
        let parent = metadata_parent(&metadata)?.ok_or_else(|| {
            Diagnostic::new(
                "VERSION_NO_PARENT",
                "active version has no parent to roll back to",
                json!({ "hash": current }),
            )
        })?;
        self.version_metadata_unlocked(&parent)?;
        self.version_source_unlocked(&parent)?;
        self.write_active_pointer(&parent)?;
        self.append_event(&json!({
            "event": "rolled-back",
            "from": current,
            "to": parent,
            "at": at,
        }))?;
        Ok(parent)
    }

    fn active_hash_unlocked(&self) -> YanshuResult<Option<String>> {
        let path = self.root.join("active.json");
        if !path.exists() {
            return Ok(None);
        }
        let document = read_json(&path)?;
        let active = document
            .as_object()
            .and_then(|object| object.get("active"))
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid_store(&path))?;
        validate_hash(active)?;
        Ok(Some(active.to_owned()))
    }

    fn version_source_unlocked(&self, hash: &str) -> YanshuResult<String> {
        let path = self.version_source_path(hash);
        if !path.is_file() {
            return Err(unknown_version(hash));
        }
        let source = fs::read_to_string(&path).map_err(|_| invalid_store(&path))?;
        if source_hash(&source) != hash {
            return Err(integrity_failure(hash));
        }
        Ok(source)
    }

    fn version_metadata_unlocked(&self, hash: &str) -> YanshuResult<JsonValue> {
        let path = self.version_metadata_path(hash);
        if !path.is_file() {
            return Err(unknown_version(hash));
        }
        let metadata = read_json(&path)?;
        let object = metadata.as_object().ok_or_else(|| invalid_store(&path))?;
        if object.get("hash").and_then(JsonValue::as_str) != Some(hash)
            || object.get("program").and_then(JsonValue::as_str).is_none()
            || object
                .get("languageVersion")
                .and_then(JsonValue::as_number)
                .is_none()
            || object.get("provider").and_then(JsonValue::as_str).is_none()
            || !object.contains_key("providerMetadata")
            || object
                .get("registeredAt")
                .and_then(JsonValue::as_u64)
                .is_none()
            || object
                .get("report")
                .and_then(JsonValue::as_object)
                .is_none()
        {
            return Err(invalid_store(&path));
        }
        metadata_parent(&metadata)?;
        Ok(metadata)
    }

    fn ensure_directories(&self) -> YanshuResult<()> {
        fs::create_dir_all(self.root.join("versions"))
            .and_then(|()| fs::create_dir_all(self.root.join("metadata")))
            .map_err(|_| write_failure(&self.root))
    }

    fn write_active_pointer(&self, hash: &str) -> YanshuResult<()> {
        self.ensure_directories()?;
        write_json_atomically(&self.root.join("active.json"), &json!({ "active": hash }))
    }

    fn append_event(&self, event: &JsonValue) -> YanshuResult<()> {
        self.ensure_directories()?;
        let path = self.root.join("events.jsonl");
        let mut bytes = serde_json::to_vec(event).map_err(|_| write_failure(&path))?;
        bytes.push(b'\n');
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|_| write_failure(&path))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| write_failure(&path))
    }

    fn version_source_path(&self, hash: &str) -> PathBuf {
        self.root.join("versions").join(format!("{hash}.yan"))
    }

    fn version_metadata_path(&self, hash: &str) -> PathBuf {
        self.root.join("metadata").join(format!("{hash}.json"))
    }
}

#[must_use]
pub fn source_hash(source: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(source.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub fn atomic_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("store.json");
    let (temporary, mut file) = create_temporary_file(parent, file_name)?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ignored = fs::remove_file(&temporary);
    }
    result
}

fn create_temporary_file(parent: &Path, file_name: &str) -> io::Result<(PathBuf, fs::File)> {
    static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    for _attempt in 0..128 {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file",
    ))
}

fn write_json_atomically(path: &Path, value: &JsonValue) -> YanshuResult<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| write_failure(path))?;
    bytes.push(b'\n');
    atomic_replace(path, &bytes).map_err(|_| write_failure(path))
}

fn read_json(path: &Path) -> YanshuResult<JsonValue> {
    let source = fs::read_to_string(path).map_err(|_| invalid_store(path))?;
    serde_json::from_str(&source).map_err(|_| invalid_store(path))
}

fn metadata_parent(metadata: &JsonValue) -> YanshuResult<Option<String>> {
    match metadata.get("parent") {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(parent)) => {
            validate_hash(parent)?;
            Ok(Some(parent.clone()))
        }
        _ => Err(Diagnostic::simple(
            "VERSION_INVALID_STORE",
            "version metadata has an invalid parent",
        )),
    }
}

fn validate_hash(hash: &str) -> YanshuResult<()> {
    if hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(());
    }
    Err(Diagnostic::new(
        "VERSION_INVALID_HASH",
        "version hash must be 64 lowercase hexadecimal characters",
        json!({ "hash": hash }),
    ))
}

fn unknown_version(hash: &str) -> Diagnostic {
    Diagnostic::new(
        "VERSION_UNKNOWN",
        "version source or metadata does not exist",
        json!({ "hash": hash }),
    )
}

fn invalid_store(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_INVALID_STORE",
        "version store file is malformed",
        json!({ "path": path.display().to_string() }),
    )
}

fn integrity_failure(hash: &str) -> Diagnostic {
    Diagnostic::new(
        "VERSION_INTEGRITY_FAILURE",
        "version source does not match its content hash",
        json!({ "hash": hash }),
    )
}

fn write_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_WRITE_FAILURE",
        "version store file could not be synchronized and replaced",
        json!({ "path": path.display().to_string() }),
    )
}

fn lock_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_LOCK_FAILURE",
        "version store lock is unavailable",
        json!({ "path": path.display().to_string() }),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
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
        let hash =
            require(store.register_candidate(registration(CANDIDATE_SOURCE, None, &passing, 1)));
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
        let events = fs::read_to_string(temporary.path.join("events.jsonl"))
            .unwrap_or_else(|error| panic!("events read failed: {error}"));
        assert_eq!(events.lines().count(), 1);
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

        let events = fs::read_to_string(temporary.path.join("events.jsonl"))
            .unwrap_or_else(|error| panic!("event read failed: {error}"));
        assert_eq!(events.lines().count(), 5);
    }

    #[test]
    fn failed_reports_and_parent_mismatches_cannot_be_promoted() {
        let temporary = TestDirectory::new();
        let store = VersionStore::new(&temporary.path);
        let failed = json!({ "passed": false });
        let hash =
            require(store.register_candidate(registration(INITIAL_SOURCE, None, &failed, 1)));
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
        let hash =
            require(store.register_candidate(registration(INITIAL_SOURCE, None, &passing, 1)));
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

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir()
                .join(format!("ai-lang-rust-store-{}-{nonce}", std::process::id()));
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
}
