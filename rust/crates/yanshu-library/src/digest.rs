#![forbid(unsafe_code)]

use sha2::{Digest, Sha256, Sha512};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryValue};

#[derive(Debug, Default)]
pub struct RustDigestBackend;

impl LibraryBackend for RustDigestBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "digest".to_owned(),
            version: 1,
            operations: vec!["sha256-text".to_owned(), "sha512-text".to_owned()],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        let [LibraryValue::String(input)] = arguments else {
            return Err(Diagnostic::simple(
                "RUST_DIGEST_BACKEND_TYPE",
                "Rust digest backend received an invalid string",
            ));
        };
        let output = match operation {
            "sha256-text" => encode_hex(&Sha256::digest(input.as_bytes())),
            "sha512-text" => encode_hex(&Sha512::digest(input.as_bytes())),
            _ => {
                return Err(Diagnostic::simple(
                    "RUST_DIGEST_BACKEND_OPERATION",
                    "Rust digest backend received an unknown operation",
                ));
            }
        };
        Ok(LibraryValue::String(output))
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}
