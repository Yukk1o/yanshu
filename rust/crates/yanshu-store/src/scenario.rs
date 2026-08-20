#![forbid(unsafe_code)]

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Map, Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{CandidateRegistration, VersionStore, source_hash};

pub fn run_version_scenario(
    initial_path: impl AsRef<Path>,
    candidate_path: impl AsRef<Path>,
) -> YanshuResult<JsonValue> {
    let initial_source = read_source(initial_path.as_ref())?;
    let candidate_source = read_source(candidate_path.as_ref())?;
    let temporary = TemporaryDirectory::new()?;
    let store = VersionStore::new(&temporary.path);
    let report = json!({ "passed": true });
    let provider_metadata = json!({});

    let initial_hash = store.register_candidate(CandidateRegistration {
        source: &initial_source,
        parent: None,
        provider: "version-conformance",
        provider_metadata: &provider_metadata,
        report: &report,
        registered_at: 1_700_000_000,
    })?;
    store.promote(&initial_hash, 1_700_000_001)?;
    let candidate_hash = store.register_candidate(CandidateRegistration {
        source: &candidate_source,
        parent: Some(&initial_hash),
        provider: "version-conformance",
        provider_metadata: &provider_metadata,
        report: &report,
        registered_at: 1_700_000_002,
    })?;
    store.promote(&candidate_hash, 1_700_000_003)?;
    let promoted_active = store.active_hash()?;
    let initial_metadata = normalize_metadata(store.version_metadata(&initial_hash)?)?;
    let candidate_metadata = normalize_metadata(store.version_metadata(&candidate_hash)?)?;
    let rolled_back = store.rollback(1_700_000_004)?;
    let rollback_active = store.active_hash()?;
    let active_source_hash = source_hash(&store.active_source()?);
    let events = read_event_names(&temporary.path.join("events.jsonl"))?;
    let expected_events = vec![
        "registered",
        "promoted",
        "registered",
        "promoted",
        "rolled-back",
    ];
    let passed = promoted_active.as_deref() == Some(candidate_hash.as_str())
        && rolled_back == initial_hash
        && rollback_active.as_deref() == Some(initial_hash.as_str())
        && active_source_hash == initial_hash
        && events == expected_events;

    Ok(json!({
        "formatVersion": 1,
        "passed": passed,
        "initialHash": initial_hash,
        "candidateHash": candidate_hash,
        "promotedActive": promoted_active,
        "rollbackActive": rollback_active,
        "activeSourceHash": active_source_hash,
        "metadata": [initial_metadata, candidate_metadata],
        "events": events,
    }))
}

fn read_source(path: &Path) -> YanshuResult<String> {
    fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "VERSION_SCENARIO_SOURCE_READ",
            "version scenario source could not be read",
            json!({ "path": path.display().to_string(), "kind": error.kind().to_string() }),
        )
    })
}

fn normalize_metadata(metadata: JsonValue) -> YanshuResult<JsonValue> {
    let document = metadata.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "VERSION_SCENARIO_INVALID_METADATA",
            "version scenario metadata is not an object",
        )
    })?;
    let mut normalized = Map::new();
    for key in [
        "hash",
        "parent",
        "program",
        "languageVersion",
        "provider",
        "providerMetadata",
        "report",
    ] {
        let value = document.get(key).cloned().ok_or_else(|| {
            Diagnostic::new(
                "VERSION_SCENARIO_INVALID_METADATA",
                "version scenario metadata is missing a field",
                json!({ "field": key }),
            )
        })?;
        normalized.insert(key.to_owned(), value);
    }
    Ok(JsonValue::Object(normalized))
}

fn read_event_names(path: &Path) -> YanshuResult<Vec<String>> {
    let source = fs::read_to_string(path).map_err(|_| {
        Diagnostic::simple(
            "VERSION_SCENARIO_EVENT_READ",
            "version scenario event log could not be read",
        )
    })?;
    source
        .lines()
        .map(|line| {
            let event: JsonValue = serde_json::from_str(line).map_err(|_| {
                Diagnostic::simple(
                    "VERSION_SCENARIO_EVENT_READ",
                    "version scenario event log is malformed",
                )
            })?;
            event
                .get("event")
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    Diagnostic::simple(
                        "VERSION_SCENARIO_EVENT_READ",
                        "version scenario event is missing its name",
                    )
                })
        })
        .collect()
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new() -> YanshuResult<Self> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        for _attempt in 0..128 {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ai-lang-version-scenario-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(Diagnostic::new(
                        "VERSION_SCENARIO_TEMPORARY_DIRECTORY",
                        "version scenario temporary directory could not be created",
                        json!({ "kind": error.kind().to_string() }),
                    ));
                }
            }
        }
        Err(Diagnostic::simple(
            "VERSION_SCENARIO_TEMPORARY_DIRECTORY",
            "version scenario could not allocate a temporary directory",
        ))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.path);
    }
}
