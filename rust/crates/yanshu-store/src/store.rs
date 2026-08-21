#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::{Program, load_program_source};

use crate::{
    events::{EventLog, VersionEvent, invalid_events, parse_event_log},
    integrity_failure, invalid_store, lock_failure,
    metadata::{
        read_json_bounded, report_passed, validate_metadata, validate_metadata_shape,
        write_json_atomically,
    },
    metadata_parent, source_hash,
    storage::{
        MAXIMUM_ACTIVE_BYTES, MAXIMUM_EVENT_LOG_BYTES, MAXIMUM_METADATA_BYTES,
        MAXIMUM_SOURCE_BYTES, cleanup_stale_temporary_files, read_bounded,
        sync_directory_and_parent,
    },
    transaction::PendingTransaction,
    unknown_version, validate_hash, write_failure,
};

const JOURNAL_FILE: &str = ".yanshu-store.pending.json";

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
    #[cfg(test)]
    failure_after_step: Option<u8>,
}

impl VersionStore {
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            lock_timeout: Duration::from_secs(5),
            #[cfg(test)]
            failure_after_step: None,
        }
    }

    #[must_use]
    pub fn with_lock_timeout(root: impl AsRef<Path>, lock_timeout: Duration) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            lock_timeout,
            #[cfg(test)]
            failure_after_step: None,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_failure_after_step(root: impl AsRef<Path>, step: u8) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            lock_timeout: Duration::from_secs(5),
            failure_after_step: Some(step),
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
        self.recover_before_read()?;
        self.active_hash_unlocked()
    }

    /// Completes an interrupted transaction and removes recognized stale
    /// replacement files while holding the cross-process store lock.
    pub fn recover(&self) -> YanshuResult<()> {
        self.with_write_lock(|| Ok(()))
    }

    pub fn active_source(&self) -> YanshuResult<String> {
        self.recover_before_read()?;
        let hash = self.active_hash_unlocked()?.ok_or_else(|| {
            Diagnostic::simple("VERSION_NO_ACTIVE", "version store has no active version")
        })?;
        self.version_source_unlocked(&hash)
    }

    pub fn version_source(&self, hash: &str) -> YanshuResult<String> {
        validate_hash(hash)?;
        self.recover_before_read()?;
        self.version_source_unlocked(hash)
    }

    pub fn version_metadata(&self, hash: &str) -> YanshuResult<JsonValue> {
        validate_hash(hash)?;
        self.recover_before_read()?;
        self.version_metadata_unlocked(hash)
    }

    pub fn validate_event_log(
        &self,
        bytes: &[u8],
        versions: &BTreeSet<String>,
        active: Option<&str>,
    ) -> YanshuResult<()> {
        self.recover_before_read()?;
        let log = parse_event_log(bytes.to_vec())?;
        if &log.registered != versions || log.active.as_deref() != active {
            return Err(invalid_events());
        }
        let mut metadata = BTreeMap::new();
        for hash in versions {
            let document = self.version_metadata_unlocked(hash)?;
            metadata.insert(
                hash.as_str(),
                (
                    metadata_parent(&document)?,
                    document
                        .get("provider")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    document.get("registeredAt").and_then(JsonValue::as_u64),
                    report_passed(&document),
                ),
            );
        }
        for stored in &log.events {
            match &stored.event {
                VersionEvent::Registered {
                    hash,
                    parent,
                    provider,
                    at,
                } => {
                    let Some((metadata_parent, metadata_provider, registered_at, _)) =
                        metadata.get(hash.as_str())
                    else {
                        return Err(invalid_events());
                    };
                    if metadata_parent != parent
                        || metadata_provider.as_deref() != Some(provider.as_str())
                        || *registered_at != Some(*at)
                    {
                        return Err(invalid_events());
                    }
                }
                VersionEvent::Promoted { from, to, .. } => {
                    let Some((metadata_parent, _, _, passed)) = metadata.get(to.as_str()) else {
                        return Err(invalid_events());
                    };
                    if metadata_parent != from || !passed {
                        return Err(invalid_events());
                    }
                }
                VersionEvent::RolledBack { from, to, .. } => {
                    let Some((metadata_parent, _, _, _)) = metadata.get(from.as_str()) else {
                        return Err(invalid_events());
                    };
                    if metadata_parent.as_deref() != Some(to) {
                        return Err(invalid_events());
                    }
                }
            }
        }
        Ok(())
    }

    fn with_write_lock<T>(&self, operation: impl FnOnce() -> YanshuResult<T>) -> YanshuResult<T> {
        fs::create_dir_all(&self.root)
            .and_then(|()| sync_directory_and_parent(&self.root))
            .map_err(|_| lock_failure(&self.root))?;
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
        self.recover_unlocked()?;
        cleanup_stale_temporary_files(&self.root).map_err(|_| write_failure(&self.root))?;
        operation()
    }

    fn recover_before_read(&self) -> YanshuResult<()> {
        if self.journal_path().exists() {
            self.with_write_lock(|| Ok(()))?;
        }
        Ok(())
    }

    fn register_candidate_unlocked(
        &self,
        registration: CandidateRegistration<'_>,
        program: &Program,
    ) -> YanshuResult<String> {
        let hash = source_hash(registration.source);
        self.ensure_directories()?;
        let log = self.load_event_log_unlocked()?;
        self.require_active_matches(&log)?;
        let source_path = self.version_source_path(&hash);
        let metadata_path = self.version_metadata_path(&hash);

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
        validate_metadata(&metadata, registration.source, program)?;

        match (source_path.exists(), metadata_path.exists()) {
            (true, true) => {
                let existing_source = self.version_source_unlocked(&hash)?;
                if existing_source != registration.source {
                    return Err(integrity_failure(&hash));
                }
                let existing = self.version_metadata_unlocked(&hash)?;
                metadata["registeredAt"] = existing["registeredAt"].clone();
                if existing != metadata {
                    return Err(Diagnostic::new(
                        "VERSION_REGISTRATION_CONFLICT",
                        "content-addressed source already has different immutable metadata",
                        json!({ "hash": hash }),
                    ));
                }
                if !log.registered.contains(&hash) {
                    return Err(invalid_events());
                }
                Ok(hash)
            }
            (false, false) => {
                if log.registered.contains(&hash) {
                    return Err(invalid_events());
                }
                let event = log.next_event(json!({
                    "event": "registered",
                    "hash": hash,
                    "parent": registration.parent,
                    "provider": registration.provider,
                    "at": registration.registered_at,
                }))?;
                let transaction = PendingTransaction::Register {
                    source: registration.source.to_owned(),
                    metadata,
                    event,
                };
                self.commit_transaction(&transaction)?;
                Ok(hash)
            }
            _ => Err(Diagnostic::simple(
                "VERSION_INVALID_STORE",
                "version source and metadata must be committed together",
            )),
        }
    }

    fn promote_unlocked(&self, hash: &str, at: u64) -> YanshuResult<String> {
        let metadata = self.version_metadata_unlocked(hash)?;
        self.version_source_unlocked(hash)?;
        if !report_passed(&metadata) {
            return Err(Diagnostic::new(
                "VERSION_TESTS_NOT_PASSED",
                "candidate cannot be promoted before its test report passes",
                json!({ "hash": hash }),
            ));
        }
        let log = self.load_event_log_unlocked()?;
        let current = self.require_active_matches(&log)?;
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
        if !log.registered.contains(hash) {
            return Err(invalid_events());
        }
        let event = log.next_event(json!({
            "event": "promoted",
            "from": current,
            "to": hash,
            "at": at,
        }))?;
        self.commit_transaction(&PendingTransaction::Activate {
            from: current,
            to: hash.to_owned(),
            event,
        })?;
        Ok(hash.to_owned())
    }

    fn rollback_unlocked(&self, at: u64) -> YanshuResult<String> {
        let log = self.load_event_log_unlocked()?;
        let current = self.require_active_matches(&log)?.ok_or_else(|| {
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
        if !log.registered.contains(&parent) {
            return Err(invalid_events());
        }
        let event = log.next_event(json!({
            "event": "rolled-back",
            "from": current,
            "to": parent,
            "at": at,
        }))?;
        self.commit_transaction(&PendingTransaction::Activate {
            from: Some(current),
            to: parent.clone(),
            event,
        })?;
        Ok(parent)
    }

    pub(crate) fn require_active_matches(&self, log: &EventLog) -> YanshuResult<Option<String>> {
        let active = self.active_hash_unlocked()?;
        if active != log.active {
            return Err(invalid_events());
        }
        Ok(active)
    }

    pub(crate) fn load_event_log_unlocked(&self) -> YanshuResult<EventLog> {
        let path = self.root.join("events.jsonl");
        if !path.exists() {
            return Ok(EventLog::empty());
        }
        let bytes =
            read_bounded(&path, MAXIMUM_EVENT_LOG_BYTES).map_err(|_| invalid_store(&path))?;
        parse_event_log(bytes)
    }

    pub(crate) fn active_hash_unlocked(&self) -> YanshuResult<Option<String>> {
        let path = self.root.join("active.json");
        if !path.exists() {
            return Ok(None);
        }
        let document = read_json_bounded(&path, MAXIMUM_ACTIVE_BYTES)?;
        let active = document
            .as_object()
            .filter(|object| object.len() == 1)
            .and_then(|object| object.get("active"))
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid_store(&path))?;
        validate_hash(active)?;
        Ok(Some(active.to_owned()))
    }

    pub(crate) fn version_source_unlocked(&self, hash: &str) -> YanshuResult<String> {
        let path = self.version_source_path(hash);
        if !path.is_file() {
            return Err(unknown_version(hash));
        }
        let bytes = read_bounded(&path, MAXIMUM_SOURCE_BYTES).map_err(|_| invalid_store(&path))?;
        let source = String::from_utf8(bytes).map_err(|_| invalid_store(&path))?;
        if source_hash(&source) != hash {
            return Err(integrity_failure(hash));
        }
        Ok(source)
    }

    pub(crate) fn version_metadata_unlocked(&self, hash: &str) -> YanshuResult<JsonValue> {
        let path = self.version_metadata_path(hash);
        if !path.is_file() {
            return Err(unknown_version(hash));
        }
        let metadata = read_json_bounded(&path, MAXIMUM_METADATA_BYTES)?;
        validate_metadata_shape(&metadata, hash, &path)?;
        Ok(metadata)
    }

    fn ensure_directories(&self) -> YanshuResult<()> {
        fs::create_dir_all(self.root.join("versions"))
            .and_then(|()| fs::create_dir_all(self.root.join("metadata")))
            .and_then(|()| sync_directory_and_parent(&self.root))
            .map_err(|_| write_failure(&self.root))
    }

    pub(crate) fn write_active_pointer(&self, hash: &str) -> YanshuResult<()> {
        self.ensure_directories()?;
        write_json_atomically(
            &self.root.join("active.json"),
            &json!({ "active": hash }),
            MAXIMUM_ACTIVE_BYTES,
        )
    }

    fn version_source_path(&self, hash: &str) -> PathBuf {
        self.root.join("versions").join(format!("{hash}.yan"))
    }

    fn version_metadata_path(&self, hash: &str) -> PathBuf {
        self.root.join("metadata").join(format!("{hash}.json"))
    }

    fn journal_path(&self) -> PathBuf {
        self.root.join(JOURNAL_FILE)
    }

    #[cfg(test)]
    pub(crate) fn inject_failure(&self, step: u8) -> YanshuResult<()> {
        if self.failure_after_step == Some(step) {
            Err(Diagnostic::new(
                "VERSION_INJECTED_FAILURE",
                "test injected a failure after a durable transaction step",
                json!({ "step": step }),
            ))
        } else {
            Ok(())
        }
    }

    #[cfg(not(test))]
    pub(crate) fn inject_failure(&self, _step: u8) -> YanshuResult<()> {
        Ok(())
    }
}
