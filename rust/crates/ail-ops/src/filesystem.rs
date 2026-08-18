use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    MAXIMUM_FILES,
    diagnostics::{backup_too_large, invalid_backup_path, invalid_manifest, restore_write_failure},
};

pub(crate) fn read_bounded(path: &Path, maximum: u64) -> AilResult<Vec<u8>> {
    validate_regular_file(path, maximum)?;
    fs::read(path).map_err(|_| {
        Diagnostic::new(
            "BACKUP_READ_FAILURE",
            "backup source file could not be read",
            json!({ "path": path.display().to_string() }),
        )
    })
}

pub(crate) fn validate_regular_file(path: &Path, maximum: u64) -> AilResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_backup_path(path))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_backup_path(path));
    }
    if metadata.len() > maximum {
        return Err(backup_too_large());
    }
    Ok(())
}

pub(crate) fn regular_file_present(path: &Path, maximum: u64) -> AilResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_regular_file(path, maximum)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(invalid_backup_path(path)),
    }
}

pub(crate) fn validate_directory(path: &Path) -> AilResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_backup_path(path))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_backup_path(path));
    }
    Ok(())
}

pub(crate) fn read_directory(path: &Path) -> AilResult<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    let iterator = fs::read_dir(path).map_err(|_| invalid_backup_path(path))?;
    for entry in iterator {
        if entries.len() >= MAXIMUM_FILES {
            return Err(backup_too_large());
        }
        entries.push(entry.map_err(|_| invalid_backup_path(path))?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

pub(crate) fn write_new_file(path: &Path, bytes: &[u8]) -> AilResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|_| restore_write_failure(path))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| restore_write_failure(path))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| restore_write_failure(path))
}

pub(crate) fn create_temporary_directory(destination: &Path) -> AilResult<PathBuf> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|_| restore_write_failure(destination))?;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid_backup_path(destination))?;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    for _attempt in 0..128 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(restore_write_failure(&candidate)),
        }
    }
    Err(restore_write_failure(destination))
}

pub(crate) fn collect_payload_paths(root: &Path) -> AilResult<BTreeSet<String>> {
    validate_directory(root)?;
    let mut output = BTreeSet::new();
    collect_payload_directory(root, root, 0, &mut output)?;
    Ok(output)
}

fn collect_payload_directory(
    root: &Path,
    current: &Path,
    depth: usize,
    output: &mut BTreeSet<String>,
) -> AilResult<()> {
    if depth > 4 || output.len() > MAXIMUM_FILES {
        return Err(backup_too_large());
    }
    for entry in read_directory(current)? {
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| invalid_manifest())?;
        if metadata.file_type().is_symlink() {
            return Err(Diagnostic::simple(
                "BACKUP_SYMLINK_FORBIDDEN",
                "backup trees cannot contain symbolic links",
            ));
        }
        if metadata.is_dir() {
            collect_payload_directory(root, &entry.path(), depth + 1, output)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| invalid_manifest())?
                .components()
                .map(|component| component.as_os_str().to_str().ok_or_else(invalid_manifest))
                .collect::<AilResult<Vec<_>>>()?
                .join("/");
            validate_manifest_path(&relative)?;
            if !output.insert(relative) || output.len() > MAXIMUM_FILES {
                return Err(invalid_manifest());
            }
        } else {
            return Err(invalid_manifest());
        }
    }
    Ok(())
}

pub(crate) fn validate_manifest_path(relative: &str) -> AilResult<()> {
    if !(relative.starts_with("code/") || relative == "data/store.json") {
        return Err(invalid_manifest());
    }
    validate_safe_relative(relative)
}

pub(crate) fn join_safe_relative(root: &Path, relative: &str) -> AilResult<PathBuf> {
    validate_safe_relative(relative)?;
    Ok(relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

fn validate_safe_relative(relative: &str) -> AilResult<()> {
    if relative.contains('\\')
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(invalid_manifest());
    }
    Ok(())
}

pub(crate) fn reject_existing(path: &Path, code: &'static str) -> AilResult<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Diagnostic::new(
            code,
            "operation refuses to overwrite an existing path",
            json!({ "path": path.display().to_string() }),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(invalid_backup_path(path)),
    }
}

pub(crate) fn reject_destination_inside_source(source: &Path, destination: &Path) -> AilResult<()> {
    let source = fs::canonicalize(source).map_err(|_| invalid_backup_path(source))?;
    let destination = future_absolute_path(destination)?;
    if destination.starts_with(source) {
        return Err(Diagnostic::simple(
            "BACKUP_PATH_OVERLAP",
            "backup destination cannot be inside the source code store",
        ));
    }
    Ok(())
}

pub(crate) fn reject_restore_overlap(
    snapshot: &Path,
    code_store: &Path,
    data_store: &Path,
) -> AilResult<()> {
    let snapshot = fs::canonicalize(snapshot).map_err(|_| invalid_backup_path(snapshot))?;
    let code = future_absolute_path(code_store)?;
    let data = future_absolute_path(data_store)?;
    if code.starts_with(&snapshot)
        || data.starts_with(&snapshot)
        || data.starts_with(&code)
        || code.starts_with(&data)
    {
        return Err(Diagnostic::simple(
            "RESTORE_PATH_OVERLAP",
            "restore targets cannot overlap the snapshot or each other",
        ));
    }
    Ok(())
}

fn future_absolute_path(path: &Path) -> AilResult<PathBuf> {
    let name = path.file_name().ok_or_else(|| invalid_backup_path(path))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|_| invalid_backup_path(parent))?;
    }
    let parent = fs::canonicalize(parent).map_err(|_| invalid_backup_path(parent))?;
    Ok(parent.join(name))
}

pub(crate) fn file_name(path: &Path) -> AilResult<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| invalid_backup_path(path))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(crate) fn timestamp_milliseconds() -> u64 {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(value).unwrap_or(u64::MAX)
}
