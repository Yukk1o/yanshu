#![forbid(unsafe_code)]

mod base64;
mod hex;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue};
use base64::{decode_base64_text, encode_base64_text};
use hex::{decode_hex_text, encode_hex_text};

pub const MAXIMUM_ENCODING_INPUT_BYTES: usize = 1024 * 1024;
pub const MAXIMUM_ENCODING_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingOperation {
    Base64EncodeText,
    Base64DecodeText,
    HexEncodeText,
    HexDecodeText,
}

#[derive(Debug, Default)]
pub struct RustEncodingBackend;

impl LibraryBackend for RustEncodingBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "encoding".to_owned(),
            version: 1,
            operations: vec![
                "base64-encode-text".to_owned(),
                "base64-decode-text".to_owned(),
                "hex-encode-text".to_owned(),
                "hex-decode-text".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        let [LibraryValue::String(input)] = arguments else {
            return Err(Diagnostic::simple(
                "RUST_ENCODING_BACKEND_TYPE",
                "Rust encoding backend received an invalid string",
            ));
        };
        let result = match operation {
            "base64-encode-text" => encode_base64_text(input),
            "base64-decode-text" => decode_base64_text(input),
            "hex-encode-text" => encode_hex_text(input),
            "hex-decode-text" => decode_hex_text(input),
            _ => {
                return Err(Diagnostic::simple(
                    "RUST_ENCODING_BACKEND_OPERATION",
                    "Rust encoding backend received an unknown operation",
                ));
            }
        };
        Ok(match result {
            Ok(value) => LibraryValue::Ok(Box::new(LibraryValue::String(value))),
            Err(issue) => LibraryValue::Err(Box::new(issue.into_value())),
        })
    }
}

#[must_use]
pub fn encoding_fuel_work(operation: EncodingOperation, arguments: &[LibraryValue]) -> u64 {
    let [LibraryValue::String(input)] = arguments else {
        return u64::MAX;
    };
    let input_bytes = u64::try_from(input.len()).unwrap_or(u64::MAX);
    let output_bytes = match operation {
        EncodingOperation::Base64EncodeText => input_bytes
            .saturating_add(2)
            .checked_div(3)
            .unwrap_or(u64::MAX)
            .saturating_mul(4),
        EncodingOperation::Base64DecodeText => input_bytes
            .checked_div(4)
            .unwrap_or(u64::MAX)
            .saturating_mul(3),
        EncodingOperation::HexEncodeText => input_bytes.saturating_mul(2),
        EncodingOperation::HexDecodeText => input_bytes.checked_div(2).unwrap_or(u64::MAX),
    };
    input_bytes.saturating_add(output_bytes)
}

fn check_input(input: &str) -> Result<(), EncodingIssue> {
    if input.len() > MAXIMUM_ENCODING_INPUT_BYTES {
        Err(EncodingIssue::input_limit())
    } else {
        Ok(())
    }
}

fn check_output(output_len: usize) -> Result<(), EncodingIssue> {
    if output_len > MAXIMUM_ENCODING_OUTPUT_BYTES {
        Err(EncodingIssue::output_limit())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EncodingIssue {
    code: &'static str,
    offset: Option<usize>,
    maximum: Option<usize>,
}

impl EncodingIssue {
    const fn invalid_base64(offset: usize) -> Self {
        Self {
            code: "ENCODING_INVALID_BASE64",
            offset: Some(offset),
            maximum: None,
        }
    }

    const fn invalid_hex(offset: usize) -> Self {
        Self {
            code: "ENCODING_INVALID_HEX",
            offset: Some(offset),
            maximum: None,
        }
    }

    const fn invalid_utf8(offset: usize) -> Self {
        Self {
            code: "ENCODING_INVALID_UTF8",
            offset: Some(offset),
            maximum: None,
        }
    }

    const fn input_limit() -> Self {
        Self {
            code: "ENCODING_INPUT_LIMIT",
            offset: None,
            maximum: Some(MAXIMUM_ENCODING_INPUT_BYTES),
        }
    }

    const fn output_limit() -> Self {
        Self {
            code: "ENCODING_OUTPUT_LIMIT",
            offset: None,
            maximum: Some(MAXIMUM_ENCODING_OUTPUT_BYTES),
        }
    }

    fn into_value(self) -> LibraryValue {
        let mut fields = BTreeMap::from([(
            LibraryKey::String("code".to_owned()),
            LibraryValue::String(self.code.to_owned()),
        )]);
        if let Some(offset) = self.offset {
            fields.insert(
                LibraryKey::String("offset".to_owned()),
                LibraryValue::Int(offset.into()),
            );
        }
        if let Some(maximum) = self.maximum {
            fields.insert(
                LibraryKey::String("maximum".to_owned()),
                LibraryValue::Int(maximum.into()),
            );
        }
        LibraryValue::Map(fields)
    }
}
