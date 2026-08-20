use std::{collections::BTreeSet, path::Path};

use serde_json::Value as JsonValue;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_store::VersionStore;

use crate::{
    MAXIMUM_FILE_BYTES, MAXIMUM_FILES, MAXIMUM_TOTAL_BYTES,
    diagnostics::{backup_too_large, unexpected_store_file},
    filesystem::{
        file_name, read_bounded, read_directory, valid_hash, validate_directory,
        validate_regular_file,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct SnapshotFile {
    pub(crate) relative: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn validate_code_store(
    root: &Path,
    allow_lock: bool,
) -> YanshuResult<(Option<String>, Vec<SnapshotFile>)> {
    validate_directory(root)?;
    let store = VersionStore::new(root);
    let active = store.active_hash()?;
    let mut files = Vec::new();
    let mut source_hashes = BTreeSet::new();
    let mut metadata_hashes = BTreeSet::new();
    let mut event_log = None;
    for entry in read_directory(root)? {
        let name = file_name(&entry.path())?;
        match name.as_str() {
            "versions" => collect_version_directory(
                &store,
                &entry.path(),
                "yan",
                "versions",
                &mut source_hashes,
                &mut files,
            )?,
            "metadata" => collect_version_directory(
                &store,
                &entry.path(),
                "json",
                "metadata",
                &mut metadata_hashes,
                &mut files,
            )?,
            "active.json" => files.push(read_snapshot_file(&entry.path(), "code/active.json")?),
            "events.jsonl" => {
                let file = read_snapshot_file(&entry.path(), "code/events.jsonl")?;
                event_log = Some(file.bytes.clone());
                files.push(file);
            }
            ".yanshu-store.lock" if allow_lock => {}
            _ => return Err(unexpected_store_file(&entry.path())),
        }
        if files.len() > MAXIMUM_FILES {
            return Err(backup_too_large());
        }
    }
    if source_hashes != metadata_hashes {
        return Err(Diagnostic::simple(
            "BACKUP_VERSION_SET_MISMATCH",
            "version source and metadata sets do not match",
        ));
    }
    for hash in &source_hashes {
        store.version_source(hash)?;
        store.version_metadata(hash)?;
    }
    if let Some(hash) = &active
        && !source_hashes.contains(hash)
    {
        return Err(Diagnostic::simple(
            "BACKUP_ACTIVE_VERSION_MISSING",
            "active version is not present in the version set",
        ));
    }
    validate_events(
        event_log.as_deref().unwrap_or_default(),
        &store,
        &source_hashes,
        active.as_deref(),
    )?;
    Ok((active, files))
}

pub(crate) fn enforce_snapshot_limits(files: &[SnapshotFile]) -> YanshuResult<()> {
    if files.len() > MAXIMUM_FILES {
        return Err(backup_too_large());
    }
    let mut total = 0_u64;
    for file in files {
        let length = u64::try_from(file.bytes.len()).map_err(|_| backup_too_large())?;
        if length > MAXIMUM_FILE_BYTES {
            return Err(backup_too_large());
        }
        total = total
            .checked_add(length)
            .filter(|value| *value <= MAXIMUM_TOTAL_BYTES)
            .ok_or_else(backup_too_large)?;
    }
    Ok(())
}

fn collect_version_directory(
    store: &VersionStore,
    path: &Path,
    extension: &str,
    directory: &str,
    hashes: &mut BTreeSet<String>,
    files: &mut Vec<SnapshotFile>,
) -> YanshuResult<()> {
    validate_directory(path)?;
    for entry in read_directory(path)? {
        validate_regular_file(&entry.path(), MAXIMUM_FILE_BYTES)?;
        let name = file_name(&entry.path())?;
        let suffix = format!(".{extension}");
        let hash = name
            .strip_suffix(&suffix)
            .filter(|value| valid_hash(value))
            .ok_or_else(|| unexpected_store_file(&entry.path()))?;
        if !hashes.insert(hash.to_owned()) {
            return Err(unexpected_store_file(&entry.path()));
        }
        if directory == "versions" {
            store.version_source(hash)?;
        } else {
            store.version_metadata(hash)?;
        }
        files.push(read_snapshot_file(
            &entry.path(),
            &format!("code/{directory}/{name}"),
        )?);
        if files.len() > MAXIMUM_FILES {
            return Err(backup_too_large());
        }
    }
    Ok(())
}

fn read_snapshot_file(path: &Path, relative: &str) -> YanshuResult<SnapshotFile> {
    Ok(SnapshotFile {
        relative: relative.to_owned(),
        bytes: read_bounded(path, MAXIMUM_FILE_BYTES)?,
    })
}

fn validate_events(
    bytes: &[u8],
    store: &VersionStore,
    versions: &BTreeSet<String>,
    active: Option<&str>,
) -> YanshuResult<()> {
    let source = std::str::from_utf8(bytes).map_err(|_| {
        Diagnostic::simple(
            "BACKUP_INVALID_EVENTS",
            "version event log is not valid UTF-8",
        )
    })?;
    let mut current: Option<String> = None;
    let mut registered = BTreeSet::new();
    for line in source.lines() {
        let event: JsonValue = serde_json::from_str(line).map_err(|_| invalid_events())?;
        let object = event.as_object().ok_or_else(invalid_events)?;
        match object.get("event").and_then(JsonValue::as_str) {
            Some("registered") if object.len() == 5 => {
                let hash = event_hash(object.get("hash"), versions)?;
                let parent = event_optional_hash(object.get("parent"), versions)?;
                if object.get("provider").and_then(JsonValue::as_str).is_none()
                    || object.get("at").and_then(JsonValue::as_u64).is_none()
                    || metadata_parent(store, hash)?.as_deref() != parent
                {
                    return Err(invalid_events());
                }
                registered.insert(hash.to_owned());
            }
            Some("promoted") if object.len() == 4 => {
                let from = event_optional_hash(object.get("from"), versions)?;
                let to = event_hash(object.get("to"), versions)?;
                if object.get("at").and_then(JsonValue::as_u64).is_none()
                    || from != current.as_deref()
                    || !registered.contains(to)
                    || metadata_parent(store, to)?.as_deref() != from
                {
                    return Err(invalid_events());
                }
                current = Some(to.to_owned());
            }
            Some("rolled-back") if object.len() == 4 => {
                let from = event_hash(object.get("from"), versions)?;
                let to = event_hash(object.get("to"), versions)?;
                if object.get("at").and_then(JsonValue::as_u64).is_none()
                    || current.as_deref() != Some(from)
                    || !registered.contains(to)
                    || metadata_parent(store, from)?.as_deref() != Some(to)
                {
                    return Err(invalid_events());
                }
                current = Some(to.to_owned());
            }
            _ => return Err(invalid_events()),
        }
    }
    if registered != *versions || current.as_deref() != active {
        return Err(invalid_events());
    }
    Ok(())
}

fn event_hash<'value>(
    value: Option<&'value JsonValue>,
    versions: &BTreeSet<String>,
) -> YanshuResult<&'value str> {
    value
        .and_then(JsonValue::as_str)
        .filter(|hash| versions.contains(*hash))
        .ok_or_else(invalid_events)
}

fn event_optional_hash<'value>(
    value: Option<&'value JsonValue>,
    versions: &BTreeSet<String>,
) -> YanshuResult<Option<&'value str>> {
    match value {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(hash)) if versions.contains(hash) => Ok(Some(hash)),
        _ => Err(invalid_events()),
    }
}

fn metadata_parent(store: &VersionStore, hash: &str) -> YanshuResult<Option<String>> {
    let metadata = store.version_metadata(hash)?;
    match metadata.get("parent") {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(parent)) => Ok(Some(parent.clone())),
        _ => Err(invalid_events()),
    }
}

fn invalid_events() -> Diagnostic {
    Diagnostic::simple(
        "BACKUP_INVALID_EVENTS",
        "version event log is not a complete valid lifecycle",
    )
}
