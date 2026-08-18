use std::path::Path;

use ail_diagnostic::Diagnostic;
use serde_json::json;

pub(crate) fn invalid_manifest() -> Diagnostic {
    Diagnostic::simple("BACKUP_INVALID_MANIFEST", "backup manifest is invalid")
}

pub(crate) fn invalid_backup_path(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "BACKUP_INVALID_PATH",
        "backup path must be a regular local file or directory",
        json!({ "path": path.display().to_string() }),
    )
}

pub(crate) fn unexpected_store_file(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "BACKUP_UNEXPECTED_STORE_FILE",
        "version store contains an unexpected file",
        json!({ "path": path.display().to_string() }),
    )
}

pub(crate) fn backup_too_large() -> Diagnostic {
    Diagnostic::simple("BACKUP_TOO_LARGE", "backup exceeded its file or byte limit")
}

pub(crate) fn restore_write_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "RESTORE_WRITE_FAILURE",
        "backup or restore file could not be written durably",
        json!({ "path": path.display().to_string() }),
    )
}

pub(crate) fn service_lock_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "SERVICE_LOCK_FAILURE",
        "service maintenance lock could not be acquired",
        json!({ "path": path.display().to_string() }),
    )
}
