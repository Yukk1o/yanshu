#![forbid(unsafe_code)]

use ail_diagnostic::{AilResult, Diagnostic};

use crate::{BackendDescriptor, LibraryBackend, LibraryValue};

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
            "replace" => Ok(LibraryValue::String(
                string_argument(arguments, 0)?.replace(
                    string_argument(arguments, 1)?,
                    string_argument(arguments, 2)?,
                ),
            )),
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
