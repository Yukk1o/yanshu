use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::json;

use crate::{diagnostics::service_lock_failure, filesystem::validate_directory};

#[derive(Debug)]
pub struct ServiceLease {
    _file: File,
}

pub fn acquire_service_lease(data_store: impl AsRef<Path>) -> AilResult<ServiceLease> {
    let path = service_lock_path(data_store.as_ref());
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|_| service_lock_failure(&path))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| service_lock_failure(&path))?;
    match file.try_lock() {
        Ok(()) => Ok(ServiceLease { _file: file }),
        Err(fs::TryLockError::WouldBlock) => Err(Diagnostic::new(
            "SERVICE_MAINTENANCE_LOCKED",
            "service data store is already held by another server or maintenance operation",
            json!({ "lock": path.display().to_string() }),
        )),
        Err(fs::TryLockError::Error(_)) => Err(service_lock_failure(&path)),
    }
}

pub(crate) fn acquire_version_lease(code_store: &Path) -> AilResult<File> {
    validate_directory(code_store)?;
    let path = code_store.join(".ail-store.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| service_lock_failure(&path))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(Diagnostic::simple(
            "BACKUP_VERSION_STORE_BUSY",
            "version store is being modified by another process",
        )),
        Err(fs::TryLockError::Error(_)) => Err(service_lock_failure(&path)),
    }
}

fn service_lock_path(data_store: &Path) -> PathBuf {
    let mut value = data_store.as_os_str().to_os_string();
    value.push(".service.lock");
    PathBuf::from(value)
}
