#![forbid(unsafe_code)]

mod parse;
mod stringify;

use std::collections::BTreeMap;

use num_bigint::BigInt;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue};

use self::parse::parse_json;
use self::stringify::stringify_canonical;
pub(crate) use self::stringify::stringify_fuel_work;

pub const MAXIMUM_JSON_INPUT_BYTES: usize = 1024 * 1024;
pub const MAXIMUM_JSON_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAXIMUM_JSON_STRING_BYTES: usize = 1024 * 1024;
pub const MAXIMUM_JSON_NODES: usize = 10_000;
pub const MAXIMUM_JSON_DEPTH: usize = 64;
pub const MAXIMUM_JSON_INTEGER_BITS: u64 = 65_536;
const MAXIMUM_JSON_INTEGER_DIGITS: usize = 20_000;

#[derive(Debug, Default)]
pub struct RustJsonBackend;

impl LibraryBackend for RustJsonBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "json".to_owned(),
            version: 1,
            operations: vec!["parse".to_owned(), "stringify-canonical".to_owned()],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        match operation {
            "parse" => {
                let [LibraryValue::String(input)] = arguments else {
                    return Err(invalid_backend_type("parse"));
                };
                Ok(as_result(parse_json(input)))
            }
            "stringify-canonical" => {
                let [value] = arguments else {
                    return Err(invalid_backend_type("stringify-canonical"));
                };
                Ok(as_result(
                    stringify_canonical(value).map(LibraryValue::String),
                ))
            }
            _ => Err(Diagnostic::simple(
                "RUST_JSON_BACKEND_OPERATION",
                "Rust JSON backend received an unknown operation",
            )),
        }
    }
}

fn invalid_backend_type(operation: &str) -> Diagnostic {
    Diagnostic::new(
        "RUST_JSON_BACKEND_TYPE",
        "Rust JSON backend received invalid arguments",
        serde_json::json!({ "operation": operation }),
    )
}

fn as_result(result: Result<LibraryValue, JsonIssue>) -> LibraryValue {
    match result {
        Ok(value) => LibraryValue::Ok(Box::new(value)),
        Err(issue) => LibraryValue::Err(Box::new(issue.into_value())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonIssue {
    code: &'static str,
    offset: Option<usize>,
    maximum: Option<usize>,
    kind: Option<&'static str>,
}

impl JsonIssue {
    const fn at(code: &'static str, offset: usize) -> Self {
        Self {
            code,
            offset: Some(offset),
            maximum: None,
            kind: None,
        }
    }

    const fn limit(code: &'static str, offset: Option<usize>, maximum: usize) -> Self {
        Self {
            code,
            offset,
            maximum: Some(maximum),
            kind: None,
        }
    }

    const fn unsupported(kind: &'static str) -> Self {
        Self {
            code: "JSON_UNSUPPORTED_VALUE",
            offset: None,
            maximum: None,
            kind: Some(kind),
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
                LibraryValue::Int(BigInt::from(offset)),
            );
        }
        if let Some(maximum) = self.maximum {
            fields.insert(
                LibraryKey::String("maximum".to_owned()),
                LibraryValue::Int(BigInt::from(maximum)),
            );
        }
        if let Some(kind) = self.kind {
            fields.insert(
                LibraryKey::String("kind".to_owned()),
                LibraryValue::String(kind.to_owned()),
            );
        }
        LibraryValue::Map(fields)
    }
}
