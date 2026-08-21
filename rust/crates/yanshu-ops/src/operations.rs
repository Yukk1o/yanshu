use std::{fs, path::Path};

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_service::FileKvStore;
use yanshu_store::VersionStore;

use crate::{
    MAXIMUM_FILE_BYTES,
    diagnostics::restore_write_failure,
    filesystem::{
        create_temporary_directory, join_safe_relative, read_bounded, regular_file_present,
        reject_destination_inside_source, reject_existing, reject_restore_overlap, write_new_file,
    },
    lease::{acquire_service_lease, acquire_version_lease},
    manifest::{Manifest, load_verified_manifest, report_json, write_snapshot},
    store::{SnapshotFile, enforce_snapshot_limits, validate_code_store},
};

pub fn create_backup(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> YanshuResult<JsonValue> {
    let code_store = code_store.as_ref();
    let data_store = data_store.as_ref();
    let destination = destination.as_ref();
    reject_existing(destination, "BACKUP_TARGET_EXISTS")?;
    reject_destination_inside_source(code_store, destination)?;

    let _service_lease = acquire_service_lease(data_store)?;
    VersionStore::new(code_store).recover()?;
    let _version_lease = acquire_version_lease(code_store)?;
    let (active_version, mut files) = validate_code_store(code_store, true)?;
    let data_store_present = regular_file_present(data_store, MAXIMUM_FILE_BYTES)?;
    if data_store_present {
        FileKvStore::open(data_store)?;
        files.push(SnapshotFile {
            relative: "data/store.json".to_owned(),
            bytes: read_bounded(data_store, MAXIMUM_FILE_BYTES)?,
        });
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    enforce_snapshot_limits(&files)?;

    let temporary = create_temporary_directory(destination)?;
    let result = (|| -> YanshuResult<Manifest> {
        write_snapshot(
            &temporary,
            &files,
            active_version.as_deref(),
            data_store_present,
        )?;
        let verified = load_verified_manifest(&temporary)?;
        verify_semantics(&temporary, &verified)?;
        commit_snapshot(&temporary, destination)?;
        Ok(verified)
    })();
    if result.is_err() {
        let _ignored = fs::remove_dir_all(&temporary);
    }
    result.map(|manifest| report_json(destination, &manifest))
}

pub fn verify_backup(snapshot: impl AsRef<Path>) -> YanshuResult<JsonValue> {
    let snapshot = snapshot.as_ref();
    let manifest = load_verified_manifest(snapshot)?;
    verify_semantics(snapshot, &manifest)?;
    Ok(report_json(snapshot, &manifest))
}

pub fn restore_backup(
    snapshot: impl AsRef<Path>,
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
) -> YanshuResult<JsonValue> {
    let snapshot = snapshot.as_ref();
    let code_store = code_store.as_ref();
    let data_store = data_store.as_ref();
    reject_existing(code_store, "RESTORE_TARGET_EXISTS")?;
    reject_existing(data_store, "RESTORE_TARGET_EXISTS")?;
    reject_restore_overlap(snapshot, code_store, data_store)?;
    let manifest = load_verified_manifest(snapshot)?;
    verify_semantics(snapshot, &manifest)?;
    let _service_lease = acquire_service_lease(data_store)?;
    reject_existing(code_store, "RESTORE_TARGET_EXISTS")?;
    reject_existing(data_store, "RESTORE_TARGET_EXISTS")?;

    let payload = snapshot.join("payload");
    let staged_code = create_temporary_directory(code_store)?;
    let staged_data = if manifest.data_store_present {
        match create_temporary_directory(data_store) {
            Ok(directory) => Some(directory),
            Err(diagnostic) => {
                let _ignored = fs::remove_dir_all(&staged_code);
                return Err(diagnostic);
            }
        }
    } else {
        None
    };
    let mut committed_data = false;
    let result = (|| -> YanshuResult<()> {
        for entry in manifest
            .entries
            .iter()
            .filter(|entry| entry.relative.starts_with("code/"))
        {
            let suffix = entry.relative.strip_prefix("code/").ok_or_else(|| {
                Diagnostic::simple("BACKUP_INVALID_MANIFEST", "backup manifest is invalid")
            })?;
            let target = join_safe_relative(&staged_code, suffix)?;
            let source = join_safe_relative(&payload, &entry.relative)?;
            write_new_file(&target, &read_bounded(&source, MAXIMUM_FILE_BYTES)?)?;
        }
        if let Some(staged_data) = &staged_data {
            let source = payload.join("data").join("store.json");
            write_new_file(
                &staged_data.join("store.json"),
                &read_bounded(&source, MAXIMUM_FILE_BYTES)?,
            )?;
        }
        let (active, _) = validate_code_store(&staged_code, false)?;
        if active != manifest.active_version {
            return Err(Diagnostic::simple(
                "BACKUP_SEMANTIC_MISMATCH",
                "restored active version does not match the verified manifest",
            ));
        }
        if let Some(staged_data) = &staged_data {
            FileKvStore::open(staged_data.join("store.json"))?;
        }

        reject_existing(code_store, "RESTORE_TARGET_EXISTS")?;
        reject_existing(data_store, "RESTORE_TARGET_EXISTS")?;
        if let Some(staged_data) = &staged_data {
            fs::rename(staged_data.join("store.json"), data_store)
                .map_err(|_| restore_write_failure(data_store))?;
            committed_data = true;
            fs::remove_dir(staged_data).map_err(|_| restore_write_failure(staged_data))?;
        }
        fs::rename(&staged_code, code_store).map_err(|_| restore_write_failure(code_store))?;
        Ok(())
    })();
    if let Err(diagnostic) = result {
        if committed_data {
            let _ignored = fs::remove_file(data_store);
        }
        let _ignored = fs::remove_dir_all(&staged_code);
        if let Some(staged_data) = &staged_data {
            let _ignored = fs::remove_dir_all(staged_data);
        }
        return Err(diagnostic);
    }
    Ok(json!({
        "ok": true,
        "snapshot": snapshot.display().to_string(),
        "restored": {
            "codeStore": code_store.display().to_string(),
            "dataStore": data_store.display().to_string(),
            "activeVersion": manifest.active_version,
            "dataStorePresent": manifest.data_store_present,
        }
    }))
}

fn verify_semantics(root: &Path, manifest: &Manifest) -> YanshuResult<()> {
    let payload = root.join("payload");
    let (verified_active, _) = validate_code_store(&payload.join("code"), false)?;
    if verified_active != manifest.active_version {
        return Err(Diagnostic::simple(
            "BACKUP_SEMANTIC_MISMATCH",
            "backup active version does not match its verified code store",
        ));
    }
    if manifest.data_store_present {
        FileKvStore::open(payload.join("data").join("store.json"))?;
    }
    Ok(())
}

fn commit_snapshot(temporary: &Path, destination: &Path) -> YanshuResult<()> {
    reject_existing(destination, "BACKUP_TARGET_EXISTS")?;
    fs::create_dir(destination).map_err(|_| restore_write_failure(destination))?;
    let result = fs::rename(temporary.join("payload"), destination.join("payload"))
        .and_then(|()| {
            fs::rename(
                temporary.join("manifest.json"),
                destination.join("manifest.json"),
            )
        })
        .and_then(|()| fs::remove_dir(temporary));
    if result.is_err() {
        let _ignored = fs::remove_dir_all(destination);
        return Err(Diagnostic::new(
            "BACKUP_COMMIT_FAILURE",
            "verified backup directory could not be committed",
            json!({ "destination": destination.display().to_string() }),
        ));
    }
    Ok(())
}
