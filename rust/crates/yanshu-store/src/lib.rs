#![forbid(unsafe_code)]

mod events;
mod metadata;
mod recovery;
mod scenario;
mod storage;
mod store;
mod transaction;

pub use scenario::run_version_scenario;
pub use storage::atomic_replace;
pub use store::{CandidateRegistration, VersionStore};

use std::path::Path;

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

#[must_use]
pub fn source_hash(source: &str) -> String {
    storage::sha256_hex(source.as_bytes())
}

pub(crate) fn metadata_parent(metadata: &JsonValue) -> YanshuResult<Option<String>> {
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

pub(crate) fn validate_hash(hash: &str) -> YanshuResult<()> {
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

pub(crate) fn unknown_version(hash: &str) -> Diagnostic {
    Diagnostic::new(
        "VERSION_UNKNOWN",
        "version source or metadata does not exist",
        json!({ "hash": hash }),
    )
}

pub(crate) fn invalid_store(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_INVALID_STORE",
        "version store file is malformed or exceeds its byte limit",
        json!({ "path": path.display().to_string() }),
    )
}

pub(crate) fn integrity_failure(hash: &str) -> Diagnostic {
    Diagnostic::new(
        "VERSION_INTEGRITY_FAILURE",
        "version source does not match its content hash",
        json!({ "hash": hash }),
    )
}

pub(crate) fn write_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_WRITE_FAILURE",
        "version store file could not be synchronized and replaced",
        json!({ "path": path.display().to_string() }),
    )
}

pub(crate) fn lock_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "VERSION_LOCK_FAILURE",
        "version store lock is unavailable",
        json!({ "path": path.display().to_string() }),
    )
}

#[cfg(test)]
mod tests;
