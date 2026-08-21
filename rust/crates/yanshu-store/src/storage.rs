#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

pub(crate) const MAXIMUM_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAXIMUM_METADATA_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAXIMUM_ACTIVE_BYTES: u64 = 64 * 1024;
pub(crate) const MAXIMUM_EVENT_BYTES: usize = 16 * 1024;
pub(crate) const MAXIMUM_EVENT_LOG_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAXIMUM_EVENTS: usize = 65_536;
const MAXIMUM_DIRECTORY_ENTRIES: usize = MAXIMUM_EVENTS + 1_024;
// A maximum-size source can expand up to six times when encoded as a JSON
// string; metadata is already bounded in its serialized form.
pub(crate) const MAXIMUM_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;

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
        fs::rename(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ignored = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn read_bounded(path: &Path, maximum: u64) -> io::Result<Vec<u8>> {
    let path_metadata = fs::symlink_metadata(path)?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store path is not a regular non-symlink file",
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store file is not regular or exceeds its byte limit",
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len().min(maximum)).unwrap_or(0));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store file grew beyond its byte limit while being read",
        ));
    }
    Ok(bytes)
}

pub(crate) fn remove_durably(path: &Path) -> io::Result<()> {
    fs::remove_file(path)?;
    sync_directory(path.parent().unwrap_or_else(|| Path::new(".")))
}

pub(crate) fn cleanup_stale_temporary_files(root: &Path) -> io::Result<()> {
    cleanup_temporary_files_in(root, TemporaryDirectory::Root)?;
    cleanup_temporary_files_in(&root.join("versions"), TemporaryDirectory::Versions)?;
    cleanup_temporary_files_in(&root.join("metadata"), TemporaryDirectory::Metadata)
}

pub(crate) fn sync_directory_and_parent(path: &Path) -> io::Result<()> {
    sync_directory(path)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        sync_directory(parent)?;
    }
    Ok(())
}

#[must_use]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn create_temporary_file(parent: &Path, file_name: &str) -> io::Result<(PathBuf, File)> {
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

#[derive(Clone, Copy)]
enum TemporaryDirectory {
    Root,
    Versions,
    Metadata,
}

fn cleanup_temporary_files_in(path: &Path, directory: TemporaryDirectory) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut removed = false;
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        if index >= MAXIMUM_DIRECTORY_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "version store directory exceeds its structural limit",
            ));
        }
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_internal_temporary_name(&name, directory) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() && !metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "version store temporary path is not a file",
            ));
        }
        fs::remove_file(entry.path())?;
        removed = true;
    }
    if removed {
        sync_directory(path)?;
    }
    Ok(())
}

fn is_internal_temporary_name(name: &str, directory: TemporaryDirectory) -> bool {
    let Some(body) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((body, sequence)) = body.rsplit_once('.') else {
        return false;
    };
    let Some((target, process)) = body.rsplit_once('.') else {
        return false;
    };
    if !decimal_component(process) || !decimal_component(sequence) {
        return false;
    }
    match directory {
        TemporaryDirectory::Root => matches!(
            target,
            "active.json" | "events.jsonl" | ".yanshu-store.pending.json"
        ),
        TemporaryDirectory::Versions => target.strip_suffix(".yan").is_some_and(lowercase_sha256),
        TemporaryDirectory::Metadata => target.strip_suffix(".json").is_some_and(lowercase_sha256),
    }
}

fn decimal_component(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    // Safe std does not expose the directory-handle flags required by Windows.
    // Every file is still sync_all'ed before an atomic rename; directory fsync
    // is additionally performed on Unix without adding first-party unsafe code.
    Ok(())
}
