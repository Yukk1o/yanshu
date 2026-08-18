#![forbid(unsafe_code)]

use std::{collections::BTreeSet, path::Path};

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub(crate) fn object<'a>(value: &'a Value, kind: &str) -> AilResult<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "PACKAGE_INVALID_DOCUMENT",
            format!("{kind} must be one JSON object"),
        )
    })
}

pub(crate) fn exact_fields(
    value: &Map<String, Value>,
    expected: &[&str],
    kind: &str,
) -> AilResult<()> {
    let actual = value.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(Diagnostic::new(
            "PACKAGE_INVALID_DOCUMENT",
            format!("{kind} fields do not exactly match the format"),
            json!({ "expected": expected, "actual": actual }),
        ))
    }
}

pub(crate) fn string<'a>(value: &'a Map<String, Value>, field: &str) -> AilResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Diagnostic::simple("PACKAGE_INVALID_DOCUMENT", "required string is missing"))
}

pub(crate) fn u64_field(value: &Map<String, Value>, field: &str) -> AilResult<u64> {
    value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        Diagnostic::simple("PACKAGE_INVALID_DOCUMENT", "required integer is missing")
    })
}

pub(crate) fn array<'a>(value: &'a Map<String, Value>, field: &str) -> AilResult<&'a [Value]> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| Diagnostic::simple("PACKAGE_INVALID_DOCUMENT", "required array is missing"))
}

pub(crate) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(crate) fn valid_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == &"0" || !part.starts_with('0'))
                && part.parse::<u32>().is_ok()
        })
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn valid_relative_path(value: &str, ail_only: bool) -> bool {
    if value.is_empty() || value.contains('\\') {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        && (!ail_only || path.extension().and_then(|item| item.to_str()) == Some("ail"))
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub(crate) fn hash_json(value: &Value) -> String {
    sha256(value.to_string().as_bytes())
}
