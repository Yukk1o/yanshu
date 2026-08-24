#![forbid(unsafe_code)]

use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryValue};

pub const MAXIMUM_MATH_INTEGER_BITS: u64 = 65_536;

#[derive(Debug, Default)]
pub struct RustMathBackend;

impl LibraryBackend for RustMathBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "math".to_owned(),
            version: 1,
            operations: vec![
                "abs".to_owned(),
                "clamp".to_owned(),
                "gcd".to_owned(),
                "max".to_owned(),
                "min".to_owned(),
                "sign".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        match operation {
            "abs" => Ok(LibraryValue::Int(integer_argument(arguments, 0)?.abs())),
            "sign" => {
                let value = integer_argument(arguments, 0)?;
                let sign = if value.is_zero() {
                    BigInt::ZERO
                } else if value.is_positive() {
                    BigInt::from(1_u8)
                } else {
                    BigInt::from(-1_i8)
                };
                Ok(LibraryValue::Int(sign))
            }
            "min" => {
                let left = integer_argument(arguments, 0)?;
                let right = integer_argument(arguments, 1)?;
                Ok(LibraryValue::Int(left.min(right).clone()))
            }
            "max" => {
                let left = integer_argument(arguments, 0)?;
                let right = integer_argument(arguments, 1)?;
                Ok(LibraryValue::Int(left.max(right).clone()))
            }
            "clamp" => {
                let value = integer_argument(arguments, 0)?;
                let minimum = integer_argument(arguments, 1)?;
                let maximum = integer_argument(arguments, 2)?;
                checked_clamp_bounds(minimum, maximum)?;
                Ok(LibraryValue::Int(value.max(minimum).min(maximum).clone()))
            }
            "gcd" => Ok(LibraryValue::Int(greatest_common_divisor(
                integer_argument(arguments, 0)?,
                integer_argument(arguments, 1)?,
            ))),
            _ => Err(Diagnostic::simple(
                "RUST_MATH_BACKEND_OPERATION",
                "Rust math backend received an unknown operation",
            )),
        }
    }
}

pub(crate) fn checked_integer_bits(value: &BigInt) -> YanshuResult<u64> {
    let bits = value.bits();
    if bits > MAXIMUM_MATH_INTEGER_BITS {
        return Err(Diagnostic::new(
            "RUNTIME_LIBRARY_ARGUMENT",
            "math argument exceeds the integer bit limit",
            serde_json::json!({ "maximumBits": MAXIMUM_MATH_INTEGER_BITS }),
        ));
    }
    Ok(bits)
}

pub(crate) fn checked_clamp_bounds(minimum: &BigInt, maximum: &BigInt) -> YanshuResult<()> {
    if minimum > maximum {
        return Err(Diagnostic::simple(
            "RUNTIME_LIBRARY_ARGUMENT",
            "math/clamp minimum cannot exceed maximum",
        ));
    }
    Ok(())
}

fn integer_argument(arguments: &[LibraryValue], index: usize) -> YanshuResult<&BigInt> {
    let value = arguments
        .get(index)
        .and_then(|value| match value {
            LibraryValue::Int(value) => Some(value),
            _ => None,
        })
        .ok_or_else(|| {
            Diagnostic::simple(
                "RUST_MATH_BACKEND_TYPE",
                "Rust math backend received an invalid integer",
            )
        })?;
    checked_integer_bits(value)?;
    Ok(value)
}

fn greatest_common_divisor(left: &BigInt, right: &BigInt) -> BigInt {
    let mut left = left.abs();
    let mut right = right.abs();
    while !right.is_zero() {
        let remainder = &left % &right;
        left = right;
        right = remainder;
    }
    left
}
