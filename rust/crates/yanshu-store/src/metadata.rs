#![forbid(unsafe_code)]

use std::path::Path;

use serde_json::{Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::Program;

use crate::{
    invalid_store, metadata_parent, source_hash,
    storage::{MAXIMUM_METADATA_BYTES, atomic_replace, read_bounded},
    transaction::invalid_journal,
    validate_hash, write_failure,
};

pub(crate) fn validate_metadata(
    metadata: &JsonValue,
    source: &str,
    program: &Program,
) -> YanshuResult<String> {
    let bytes = serde_json::to_vec(metadata).map_err(|_| invalid_journal())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_METADATA_BYTES {
        return Err(Diagnostic::new(
            "VERSION_METADATA_LIMIT",
            "version metadata exceeds its byte limit",
            json!({ "maximum": MAXIMUM_METADATA_BYTES }),
        ));
    }
    let object = metadata.as_object().ok_or_else(invalid_journal)?;
    let hash = object
        .get("hash")
        .and_then(JsonValue::as_str)
        .ok_or_else(invalid_journal)?;
    validate_hash(hash).map_err(|_| invalid_journal())?;
    let version = program
        .version
        .to_string()
        .parse::<u64>()
        .map_err(|_| invalid_journal())?;
    if object.len() != 8
        || source_hash(source) != hash
        || object.get("program").and_then(JsonValue::as_str) != Some(program.name.as_str())
        || object.get("languageVersion").and_then(JsonValue::as_u64) != Some(version)
        || object
            .get("provider")
            .and_then(JsonValue::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .is_none()
        || !object.contains_key("providerMetadata")
        || object
            .get("registeredAt")
            .and_then(JsonValue::as_u64)
            .is_none()
        || object
            .get("report")
            .and_then(JsonValue::as_object)
            .is_none()
    {
        return Err(invalid_journal());
    }
    metadata_parent(metadata).map_err(|_| invalid_journal())?;
    Ok(hash.to_owned())
}

pub(crate) fn validate_metadata_shape(
    metadata: &JsonValue,
    hash: &str,
    path: &Path,
) -> YanshuResult<()> {
    let object = metadata.as_object().ok_or_else(|| invalid_store(path))?;
    if object.len() != 8
        || object.get("hash").and_then(JsonValue::as_str) != Some(hash)
        || object.get("program").and_then(JsonValue::as_str).is_none()
        || object
            .get("languageVersion")
            .and_then(JsonValue::as_u64)
            .is_none()
        || object.get("provider").and_then(JsonValue::as_str).is_none()
        || !object.contains_key("providerMetadata")
        || object
            .get("registeredAt")
            .and_then(JsonValue::as_u64)
            .is_none()
        || object
            .get("report")
            .and_then(JsonValue::as_object)
            .is_none()
    {
        return Err(invalid_store(path));
    }
    metadata_parent(metadata)?;
    Ok(())
}

pub(crate) fn report_passed(metadata: &JsonValue) -> bool {
    metadata
        .get("report")
        .and_then(JsonValue::as_object)
        .and_then(|report| report.get("passed"))
        .and_then(JsonValue::as_bool)
        == Some(true)
}

pub(crate) fn write_json_atomically(
    path: &Path,
    value: &JsonValue,
    maximum: u64,
) -> YanshuResult<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| write_failure(path))?;
    bytes.push(b'\n');
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(write_failure(path));
    }
    atomic_replace(path, &bytes).map_err(|_| write_failure(path))
}

pub(crate) fn read_json_bounded(path: &Path, maximum: u64) -> YanshuResult<JsonValue> {
    let bytes = read_bounded(path, maximum).map_err(|_| invalid_store(path))?;
    serde_json::from_slice(&bytes).map_err(|_| invalid_store(path))
}
