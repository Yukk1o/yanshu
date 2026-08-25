#![forbid(unsafe_code)]

mod parse;
mod rescale;

use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue};

use self::parse::{format_scaled, parse_scaled};
use self::rescale::rescale;

pub const MAXIMUM_DECIMAL_SCALE: u64 = 1_024;
pub const MAXIMUM_DECIMAL_INPUT_BYTES: usize = 20_002;
pub const MAXIMUM_DECIMAL_OUTPUT_BYTES: usize = 20_002;
pub const MAXIMUM_DECIMAL_INTEGER_BITS: u64 = 65_536;
const MAXIMUM_DECIMAL_INTEGER_DIGITS: usize = 20_000;

#[derive(Debug, Default)]
pub struct RustDecimalBackend;

impl LibraryBackend for RustDecimalBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "decimal".to_owned(),
            version: 1,
            operations: vec![
                "parse-scaled".to_owned(),
                "format-scaled".to_owned(),
                "rescale".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        let outcome = match operation {
            "parse-scaled" => {
                let [LibraryValue::String(input), LibraryValue::Int(scale)] = arguments else {
                    return Err(invalid_backend_type(operation));
                };
                parse_scaled(input, scale).map(LibraryValue::Int)
            }
            "format-scaled" => {
                let [LibraryValue::Int(value), LibraryValue::Int(scale)] = arguments else {
                    return Err(invalid_backend_type(operation));
                };
                format_scaled(value, scale).map(LibraryValue::String)
            }
            "rescale" => {
                let [
                    LibraryValue::Int(value),
                    LibraryValue::Int(from_scale),
                    LibraryValue::Int(to_scale),
                    LibraryValue::String(rounding),
                ] = arguments
                else {
                    return Err(invalid_backend_type(operation));
                };
                rescale(value, from_scale, to_scale, rounding).map(LibraryValue::Int)
            }
            _ => {
                return Err(Diagnostic::simple(
                    "RUST_DECIMAL_BACKEND_OPERATION",
                    "Rust decimal backend received an unknown operation",
                ));
            }
        };
        Ok(as_result(outcome))
    }
}

fn invalid_backend_type(operation: &str) -> Diagnostic {
    Diagnostic::new(
        "RUST_DECIMAL_BACKEND_TYPE",
        "Rust decimal backend received invalid arguments",
        serde_json::json!({ "operation": operation }),
    )
}

fn as_result(result: Result<LibraryValue, DecimalIssue>) -> LibraryValue {
    match result {
        Ok(value) => LibraryValue::Ok(Box::new(value)),
        Err(issue) => LibraryValue::Err(Box::new(issue.into_value())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DecimalIssue {
    code: &'static str,
    offset: Option<usize>,
    maximum: Option<u64>,
}

impl DecimalIssue {
    const fn simple(code: &'static str) -> Self {
        Self {
            code,
            offset: None,
            maximum: None,
        }
    }

    const fn at(code: &'static str, offset: usize) -> Self {
        Self {
            code,
            offset: Some(offset),
            maximum: None,
        }
    }

    const fn limit(code: &'static str, maximum: u64) -> Self {
        Self {
            code,
            offset: None,
            maximum: Some(maximum),
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
        LibraryValue::Map(fields)
    }
}

fn checked_scale(value: &BigInt) -> Result<usize, DecimalIssue> {
    value
        .to_u64()
        .filter(|scale| *scale <= MAXIMUM_DECIMAL_SCALE)
        .and_then(|scale| usize::try_from(scale).ok())
        .ok_or_else(|| DecimalIssue::limit("DECIMAL_SCALE_LIMIT", MAXIMUM_DECIMAL_SCALE))
}

pub(crate) fn parse_fuel_work(input: &str, scale: &BigInt) -> u64 {
    u64::try_from(input.len())
        .unwrap_or(u64::MAX)
        .saturating_add(scale.bits())
        .saturating_add(valid_scale_work(scale, 1))
}

pub(crate) fn format_fuel_work(value: &BigInt, scale: &BigInt) -> u64 {
    value
        .bits()
        .saturating_add(scale.bits())
        .saturating_add(valid_scale_work(scale, 1))
}

pub(crate) fn rescale_fuel_work(
    value: &BigInt,
    from_scale: &BigInt,
    to_scale: &BigInt,
    rounding: &str,
) -> u64 {
    let scale_work = match (bounded_scale(from_scale), bounded_scale(to_scale)) {
        (Some(from), Some(to)) => from.abs_diff(to).saturating_mul(4),
        _ => from_scale.bits().saturating_add(to_scale.bits()),
    };
    value
        .bits()
        .saturating_add(from_scale.bits())
        .saturating_add(to_scale.bits())
        .saturating_add(scale_work)
        .saturating_add(u64::try_from(rounding.len()).unwrap_or(u64::MAX))
}

fn valid_scale_work(value: &BigInt, multiplier: u64) -> u64 {
    bounded_scale(value).map_or(0, |scale| scale.saturating_mul(multiplier))
}

fn bounded_scale(value: &BigInt) -> Option<u64> {
    value
        .to_u64()
        .filter(|scale| *scale <= MAXIMUM_DECIMAL_SCALE)
}
