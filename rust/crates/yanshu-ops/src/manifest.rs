use std::{collections::BTreeSet, fs, path::Path};

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    MAXIMUM_FILE_BYTES, MAXIMUM_FILES, MAXIMUM_MANIFEST_BYTES, MAXIMUM_TOTAL_BYTES,
    diagnostics::{backup_too_large, invalid_manifest, restore_write_failure},
    filesystem::{
        collect_payload_paths, file_name, join_safe_relative, read_bounded, read_directory,
        sha256_hex, timestamp_milliseconds, valid_hash, validate_directory, validate_manifest_path,
        validate_regular_file, write_new_file,
    },
    store::SnapshotFile,
};

#[derive(Debug, Clone)]
pub(crate) struct ManifestEntry {
    pub(crate) relative: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    pub(crate) active_version: Option<String>,
    pub(crate) data_store_present: bool,
    pub(crate) entries: Vec<ManifestEntry>,
    pub(crate) total_bytes: u64,
}

pub(crate) fn write_snapshot(
    root: &Path,
    files: &[SnapshotFile],
    active_version: Option<&str>,
    data_store_present: bool,
) -> YanshuResult<Manifest> {
    let payload = root.join("payload");
    fs::create_dir_all(payload.join("code")).map_err(|_| restore_write_failure(&payload))?;
    let mut entries = Vec::with_capacity(files.len());
    let mut total_bytes = 0_u64;
    for file in files {
        let target = join_safe_relative(&payload, &file.relative)?;
        write_new_file(&target, &file.bytes)?;
        let length = u64::try_from(file.bytes.len()).map_err(|_| backup_too_large())?;
        total_bytes = total_bytes
            .checked_add(length)
            .ok_or_else(backup_too_large)?;
        entries.push(ManifestEntry {
            relative: file.relative.clone(),
            bytes: length,
            sha256: sha256_hex(&file.bytes),
        });
    }
    let document = json!({
        "schemaVersion": 1,
        "createdAtMs": timestamp_milliseconds(),
        "activeVersion": active_version,
        "dataStorePresent": data_store_present,
        "files": entries.iter().map(|entry| json!({
            "path": entry.relative,
            "bytes": entry.bytes,
            "sha256": entry.sha256,
        })).collect::<Vec<_>>(),
    });
    let mut bytes = serde_json::to_vec(&document).map_err(|_| invalid_manifest())?;
    bytes.push(b'\n');
    write_new_file(&root.join("manifest.json"), &bytes)?;
    Ok(Manifest {
        active_version: active_version.map(str::to_owned),
        data_store_present,
        entries,
        total_bytes,
    })
}

pub(crate) fn load_verified_manifest(root: &Path) -> YanshuResult<Manifest> {
    validate_snapshot_root(root)?;
    let document: JsonValue = serde_json::from_slice(&read_bounded(
        &root.join("manifest.json"),
        MAXIMUM_MANIFEST_BYTES,
    )?)
    .map_err(|_| invalid_manifest())?;
    let object = document.as_object().ok_or_else(invalid_manifest)?;
    if object.len() != 5
        || object.get("schemaVersion").and_then(JsonValue::as_u64) != Some(1)
        || object
            .get("createdAtMs")
            .and_then(JsonValue::as_u64)
            .is_none()
    {
        return Err(invalid_manifest());
    }
    let active_version = match object.get("activeVersion") {
        Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) if valid_hash(value) => Some(value.clone()),
        _ => return Err(invalid_manifest()),
    };
    let data_store_present = object
        .get("dataStorePresent")
        .and_then(JsonValue::as_bool)
        .ok_or_else(invalid_manifest)?;
    let values = object
        .get("files")
        .and_then(JsonValue::as_array)
        .ok_or_else(invalid_manifest)?;
    if values.len() > MAXIMUM_FILES {
        return Err(backup_too_large());
    }

    let mut entries = Vec::with_capacity(values.len());
    let mut expected = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for value in values {
        let entry = value
            .as_object()
            .filter(|entry| entry.len() == 3)
            .ok_or_else(invalid_manifest)?;
        let relative = entry
            .get("path")
            .and_then(JsonValue::as_str)
            .ok_or_else(invalid_manifest)?
            .to_owned();
        validate_manifest_path(&relative)?;
        if !expected.insert(relative.clone()) {
            return Err(invalid_manifest());
        }
        let bytes = entry
            .get("bytes")
            .and_then(JsonValue::as_u64)
            .ok_or_else(invalid_manifest)?;
        let sha256 = entry
            .get("sha256")
            .and_then(JsonValue::as_str)
            .filter(|value| valid_hash(value))
            .ok_or_else(invalid_manifest)?
            .to_owned();
        if bytes > MAXIMUM_FILE_BYTES {
            return Err(backup_too_large());
        }
        total_bytes = total_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAXIMUM_TOTAL_BYTES)
            .ok_or_else(backup_too_large)?;
        entries.push(ManifestEntry {
            relative,
            bytes,
            sha256,
        });
    }
    if data_store_present != expected.contains("data/store.json") {
        return Err(invalid_manifest());
    }
    let payload = root.join("payload");
    if collect_payload_paths(&payload)? != expected {
        return Err(Diagnostic::simple(
            "BACKUP_UNEXPECTED_FILE",
            "backup payload does not exactly match its manifest",
        ));
    }
    for entry in &entries {
        let path = join_safe_relative(&payload, &entry.relative)?;
        let bytes = read_bounded(&path, MAXIMUM_FILE_BYTES)?;
        if u64::try_from(bytes.len()).ok() != Some(entry.bytes)
            || sha256_hex(&bytes) != entry.sha256
        {
            return Err(Diagnostic::new(
                "BACKUP_HASH_MISMATCH",
                "backup payload failed its size or SHA-256 check",
                json!({ "path": entry.relative }),
            ));
        }
    }
    Ok(Manifest {
        active_version,
        data_store_present,
        entries,
        total_bytes,
    })
}

pub(crate) fn report_json(path: &Path, manifest: &Manifest) -> JsonValue {
    json!({
        "ok": true,
        "snapshot": path.display().to_string(),
        "activeVersion": manifest.active_version,
        "dataStorePresent": manifest.data_store_present,
        "files": manifest.entries.len(),
        "bytes": manifest.total_bytes,
    })
}

fn validate_snapshot_root(root: &Path) -> YanshuResult<()> {
    validate_directory(root)?;
    let mut manifest_seen = false;
    let mut payload_seen = false;
    for entry in read_directory(root)? {
        match file_name(&entry.path())?.as_str() {
            "manifest.json" => {
                validate_regular_file(&entry.path(), MAXIMUM_MANIFEST_BYTES)?;
                manifest_seen = true;
            }
            "payload" => {
                validate_directory(&entry.path())?;
                payload_seen = true;
            }
            _ => return Err(invalid_manifest()),
        }
    }
    if !manifest_seen || !payload_seen {
        return Err(invalid_manifest());
    }
    Ok(())
}
