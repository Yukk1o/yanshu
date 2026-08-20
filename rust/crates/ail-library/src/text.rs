#![forbid(unsafe_code)]

use ail_diagnostic::{AilResult, Diagnostic};

use crate::{BackendDescriptor, LibraryBackend, LibraryValue};

pub(crate) const MAXIMUM_TEXT_RESULT_BYTES: usize = 1024 * 1024;

pub(crate) fn checked_replace_output_bytes(
    input: &str,
    pattern: &str,
    replacement: &str,
) -> AilResult<usize> {
    let matches = if pattern.is_empty() {
        input.chars().count().saturating_add(1)
    } else {
        input.match_indices(pattern).count()
    };
    let removed = matches.checked_mul(pattern.len()).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text replacement result exceeds the byte limit",
        )
    })?;
    let inserted = matches.checked_mul(replacement.len()).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text replacement result exceeds the byte limit",
        )
    })?;
    let output_bytes = input
        .len()
        .checked_sub(removed)
        .and_then(|remaining| remaining.checked_add(inserted))
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUNTIME_LIBRARY_RESULT_LIMIT",
                "text replacement result exceeds the byte limit",
            )
        })?;
    if output_bytes > MAXIMUM_TEXT_RESULT_BYTES {
        return Err(Diagnostic::new(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text replacement result exceeds the byte limit",
            serde_json::json!({
                "maximumBytes": MAXIMUM_TEXT_RESULT_BYTES,
                "actualBytes": output_bytes,
            }),
        ));
    }
    Ok(output_bytes)
}

#[derive(Debug, Default)]
pub struct RustTextBackend;

impl LibraryBackend for RustTextBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "text".to_owned(),
            version: 1,
            operations: vec![
                "contains?".to_owned(),
                "ends-with?".to_owned(),
                "length".to_owned(),
                "replace".to_owned(),
                "starts-with?".to_owned(),
            ],
        }
    }

    fn invoke(&mut self, operation: &str, arguments: &[LibraryValue]) -> AilResult<LibraryValue> {
        match operation {
            "length" => Ok(LibraryValue::Int(
                string_argument(arguments, 0)?.chars().count().into(),
            )),
            "starts-with?" => Ok(LibraryValue::Bool(
                string_argument(arguments, 0)?.starts_with(string_argument(arguments, 1)?),
            )),
            "ends-with?" => Ok(LibraryValue::Bool(
                string_argument(arguments, 0)?.ends_with(string_argument(arguments, 1)?),
            )),
            "contains?" => Ok(LibraryValue::Bool(
                string_argument(arguments, 0)?.contains(string_argument(arguments, 1)?),
            )),
            "replace" => {
                let input = string_argument(arguments, 0)?;
                let pattern = string_argument(arguments, 1)?;
                let replacement = string_argument(arguments, 2)?;
                checked_replace_output_bytes(input, pattern, replacement)?;
                Ok(LibraryValue::String(input.replace(pattern, replacement)))
            }
            _ => Err(Diagnostic::simple(
                "RUST_TEXT_BACKEND_OPERATION",
                "Rust text backend received an unknown operation",
            )),
        }
    }
}

fn string_argument(arguments: &[LibraryValue], index: usize) -> AilResult<&str> {
    arguments
        .get(index)
        .and_then(|value| match value {
            LibraryValue::String(text) => Some(text.as_str()),
            _ => None,
        })
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUST_TEXT_BACKEND_TYPE",
                "Rust text backend received an invalid argument",
            )
        })
}
