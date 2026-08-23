#![forbid(unsafe_code)]

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryValue};

pub(crate) const MAXIMUM_TEXT_RESULT_BYTES: usize = 1024 * 1024;
pub(crate) const MAXIMUM_TEXT_RESULT_NODES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SplitMetrics {
    pub output_bytes: usize,
    pub segments: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextByteRange {
    pub start: usize,
    pub end: usize,
}

fn check_result_bytes(actual_bytes: usize) -> YanshuResult<usize> {
    if actual_bytes > MAXIMUM_TEXT_RESULT_BYTES {
        return Err(Diagnostic::new(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text operation result exceeds the byte limit",
            serde_json::json!({
                "maximumBytes": MAXIMUM_TEXT_RESULT_BYTES,
                "actualBytes": actual_bytes,
            }),
        ));
    }
    Ok(actual_bytes)
}

pub(crate) fn checked_case_output_bytes(input: &str, uppercase: bool) -> YanshuResult<usize> {
    let output_bytes = input.chars().try_fold(0_usize, |total, character| {
        let mapped_bytes = if uppercase {
            character.to_uppercase().try_fold(0_usize, |bytes, mapped| {
                bytes.checked_add(mapped.len_utf8())
            })
        } else {
            character.to_lowercase().try_fold(0_usize, |bytes, mapped| {
                bytes.checked_add(mapped.len_utf8())
            })
        };
        mapped_bytes.and_then(|bytes| total.checked_add(bytes))
    });
    check_result_bytes(output_bytes.ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text case conversion result exceeds the byte limit",
        )
    })?)
}

pub(crate) fn checked_split_result(input: &str, separator: &str) -> YanshuResult<SplitMetrics> {
    if separator.is_empty() {
        return Err(Diagnostic::simple(
            "RUNTIME_LIBRARY_ARGUMENT",
            "text/split separator cannot be empty",
        ));
    }
    let matches = input.match_indices(separator).count();
    let segments = matches.checked_add(1).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text split result exceeds the node limit",
        )
    })?;
    let actual_nodes = segments.checked_add(1).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text split result exceeds the node limit",
        )
    })?;
    if actual_nodes > MAXIMUM_TEXT_RESULT_NODES {
        return Err(Diagnostic::new(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text split result exceeds the node limit",
            serde_json::json!({
                "maximumNodes": MAXIMUM_TEXT_RESULT_NODES,
                "actualNodes": actual_nodes,
            }),
        ));
    }
    let removed_bytes = matches.checked_mul(separator.len()).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text split result exceeds the byte limit",
        )
    })?;
    let output_bytes = input.len().checked_sub(removed_bytes).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_CONTRACT_FAILURE",
            "text split size calculation failed",
        )
    })?;
    check_result_bytes(output_bytes)?;
    Ok(SplitMetrics {
        output_bytes,
        segments,
    })
}

pub(crate) fn checked_join_output_bytes(
    values: &[LibraryValue],
    separator: &str,
) -> YanshuResult<usize> {
    let item_bytes = values.iter().try_fold(0_usize, |total, value| {
        let LibraryValue::String(value) = value else {
            return Err(Diagnostic::simple(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "text/join received a non-string list item",
            ));
        };
        total.checked_add(value.len()).ok_or_else(|| {
            Diagnostic::simple(
                "RUNTIME_LIBRARY_RESULT_LIMIT",
                "text join result exceeds the byte limit",
            )
        })
    })?;
    let separator_count = values.len().saturating_sub(1);
    let separator_bytes = separator_count
        .checked_mul(separator.len())
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUNTIME_LIBRARY_RESULT_LIMIT",
                "text join result exceeds the byte limit",
            )
        })?;
    let output_bytes = item_bytes.checked_add(separator_bytes).ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "text join result exceeds the byte limit",
        )
    })?;
    check_result_bytes(output_bytes)
}

pub(crate) fn checked_substring_byte_range(
    input: &str,
    start: &BigInt,
    end: &BigInt,
) -> YanshuResult<TextByteRange> {
    let start = start.to_usize().ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_ARGUMENT",
            "text/substring start must be a non-negative integer index",
        )
    })?;
    let end = end.to_usize().ok_or_else(|| {
        Diagnostic::simple(
            "RUNTIME_LIBRARY_ARGUMENT",
            "text/substring end must be a non-negative integer index",
        )
    })?;
    let scalar_count = input.chars().count();
    if start > end || end > scalar_count {
        return Err(Diagnostic::new(
            "RUNTIME_LIBRARY_ARGUMENT",
            "text/substring requires 0 <= start <= end <= Unicode scalar count",
            serde_json::json!({ "scalarCount": scalar_count }),
        ));
    }
    let byte_at = |index: usize| {
        if index == scalar_count {
            input.len()
        } else {
            input
                .char_indices()
                .nth(index)
                .map_or(input.len(), |(byte, _)| byte)
        }
    };
    Ok(TextByteRange {
        start: byte_at(start),
        end: byte_at(end),
    })
}

pub(crate) fn checked_replace_output_bytes(
    input: &str,
    pattern: &str,
    replacement: &str,
) -> YanshuResult<usize> {
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

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        invoke_text(operation, arguments, false)
    }
}

#[derive(Debug, Default)]
pub struct RustTextV2Backend;

impl LibraryBackend for RustTextV2Backend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "text".to_owned(),
            version: 2,
            operations: vec![
                "contains?".to_owned(),
                "ends-with?".to_owned(),
                "join".to_owned(),
                "length".to_owned(),
                "lowercase".to_owned(),
                "replace".to_owned(),
                "split".to_owned(),
                "starts-with?".to_owned(),
                "substring".to_owned(),
                "trim".to_owned(),
                "uppercase".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        invoke_text(operation, arguments, true)
    }
}

fn invoke_text(
    operation: &str,
    arguments: &[LibraryValue],
    include_v2: bool,
) -> YanshuResult<LibraryValue> {
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
        "trim" if include_v2 => {
            let trimmed = string_argument(arguments, 0)?.trim();
            check_result_bytes(trimmed.len())?;
            Ok(LibraryValue::String(trimmed.to_owned()))
        }
        "lowercase" if include_v2 => convert_case(string_argument(arguments, 0)?, false),
        "uppercase" if include_v2 => convert_case(string_argument(arguments, 0)?, true),
        "split" if include_v2 => {
            let input = string_argument(arguments, 0)?;
            let separator = string_argument(arguments, 1)?;
            let metrics = checked_split_result(input, separator)?;
            let mut values = Vec::with_capacity(metrics.segments);
            values.extend(
                input
                    .split(separator)
                    .map(|value| LibraryValue::String(value.to_owned())),
            );
            Ok(LibraryValue::List(values))
        }
        "join" if include_v2 => {
            let values = string_list_argument(arguments, 0)?;
            let separator = string_argument(arguments, 1)?;
            let output_bytes = checked_join_output_bytes(values, separator)?;
            let mut output = String::with_capacity(output_bytes);
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_str(separator);
                }
                let LibraryValue::String(value) = value else {
                    return Err(Diagnostic::simple(
                        "RUST_TEXT_BACKEND_TYPE",
                        "Rust text backend received an invalid string list",
                    ));
                };
                output.push_str(value);
            }
            Ok(LibraryValue::String(output))
        }
        "substring" if include_v2 => {
            let input = string_argument(arguments, 0)?;
            let range = checked_substring_byte_range(
                input,
                integer_argument(arguments, 1)?,
                integer_argument(arguments, 2)?,
            )?;
            Ok(LibraryValue::String(
                input[range.start..range.end].to_owned(),
            ))
        }
        _ => Err(Diagnostic::simple(
            "RUST_TEXT_BACKEND_OPERATION",
            "Rust text backend received an unknown operation",
        )),
    }
}

fn convert_case(input: &str, uppercase: bool) -> YanshuResult<LibraryValue> {
    let output_bytes = checked_case_output_bytes(input, uppercase)?;
    let mut output = String::with_capacity(output_bytes);
    for character in input.chars() {
        if uppercase {
            output.extend(character.to_uppercase());
        } else {
            output.extend(character.to_lowercase());
        }
    }
    Ok(LibraryValue::String(output))
}

fn string_argument(arguments: &[LibraryValue], index: usize) -> YanshuResult<&str> {
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

fn string_list_argument(arguments: &[LibraryValue], index: usize) -> YanshuResult<&[LibraryValue]> {
    arguments
        .get(index)
        .and_then(|value| match value {
            LibraryValue::Nil => Some([].as_slice()),
            LibraryValue::List(values) => Some(values.as_slice()),
            _ => None,
        })
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUST_TEXT_BACKEND_TYPE",
                "Rust text backend received an invalid string list",
            )
        })
}

fn integer_argument(arguments: &[LibraryValue], index: usize) -> YanshuResult<&BigInt> {
    arguments
        .get(index)
        .and_then(|value| match value {
            LibraryValue::Int(value) => Some(value),
            _ => None,
        })
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUST_TEXT_BACKEND_TYPE",
                "Rust text backend received an invalid integer",
            )
        })
}
