#![forbid(unsafe_code)]

mod format;
mod parse;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue, MAXIMUM_LIBRARY_INTEGER_BITS,
};
use format::format_radix;
use parse::{parse_decimal, parse_radix};

pub const MINIMUM_INTEGER_RADIX: u32 = 2;
pub const MAXIMUM_INTEGER_RADIX: u32 = 36;
pub const MAXIMUM_INTEGER_TEXT_BYTES: usize = 65_537;

const MAXIMUM_SIGNIFICANT_DIGITS: [usize; 35] = [
    65_536, 41_349, 32_768, 28_225, 25_353, 23_345, 21_846, 20_675, 19_729, 18_945, 18_281, 17_711,
    17_213, 16_775, 16_384, 16_034, 15_717, 15_428, 15_164, 14_921, 14_697, 14_488, 14_294, 14_113,
    13_943, 13_783, 13_633, 13_491, 13_356, 13_229, 13_108, 12_992, 12_882, 12_777, 12_677,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerTextOperation {
    ParseDecimal,
    ParseRadix,
    FormatRadix,
}

#[derive(Debug, Default)]
pub struct RustIntegerBackend;

impl LibraryBackend for RustIntegerBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "integer".to_owned(),
            version: 1,
            operations: vec![
                "format-radix".to_owned(),
                "parse-decimal".to_owned(),
                "parse-radix".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        let result = match operation {
            "parse-decimal" => {
                let [LibraryValue::String(input)] = arguments else {
                    return Err(invalid_backend_arguments());
                };
                parse_decimal(input)
            }
            "parse-radix" => {
                let [LibraryValue::String(input), LibraryValue::Int(radix)] = arguments else {
                    return Err(invalid_backend_arguments());
                };
                checked_radix(radix).and_then(|radix| parse_radix(input, radix))
            }
            "format-radix" => {
                let [LibraryValue::Int(value), LibraryValue::Int(radix)] = arguments else {
                    return Err(invalid_backend_arguments());
                };
                checked_radix(radix).and_then(|radix| format_radix(value, radix))
            }
            _ => {
                return Err(Diagnostic::simple(
                    "RUST_INTEGER_BACKEND_OPERATION",
                    "Rust integer backend received an unknown operation",
                ));
            }
        };
        Ok(match result {
            Ok(value) => LibraryValue::Ok(Box::new(value)),
            Err(issue) => LibraryValue::Err(Box::new(issue.into_value())),
        })
    }
}

#[must_use]
pub fn integer_text_fuel_work(operation: IntegerTextOperation, arguments: &[LibraryValue]) -> u64 {
    match (operation, arguments) {
        (IntegerTextOperation::ParseDecimal, [LibraryValue::String(input)]) => {
            text_complexity(input)
        }
        (
            IntegerTextOperation::ParseRadix,
            [LibraryValue::String(input), LibraryValue::Int(radix)],
        ) => text_complexity(input).saturating_add(integer_bits(radix)),
        (
            IntegerTextOperation::FormatRadix,
            [LibraryValue::Int(value), LibraryValue::Int(radix)],
        ) => magnitude_complexity(value).saturating_add(integer_bits(radix)),
        _ => u64::MAX,
    }
}

fn text_complexity(input: &str) -> u64 {
    let bytes = u64::try_from(input.len()).unwrap_or(u64::MAX).max(1);
    bytes.saturating_mul(bytes.div_ceil(64))
}

fn magnitude_complexity(value: &BigInt) -> u64 {
    let bits = integer_bits(value).max(1);
    bits.saturating_mul(bits.div_ceil(64))
}

fn integer_bits(value: &BigInt) -> u64 {
    value.bits()
}

fn checked_radix(value: &BigInt) -> Result<u32, IntegerIssue> {
    value
        .to_u32()
        .filter(|radix| (MINIMUM_INTEGER_RADIX..=MAXIMUM_INTEGER_RADIX).contains(radix))
        .ok_or_else(IntegerIssue::invalid_radix)
}

fn maximum_significant_digits(radix: u32) -> usize {
    let index = usize::try_from(radix.saturating_sub(MINIMUM_INTEGER_RADIX))
        .unwrap_or(MAXIMUM_SIGNIFICANT_DIGITS.len());
    MAXIMUM_SIGNIFICANT_DIGITS.get(index).copied().unwrap_or(0)
}

fn check_input(input: &str) -> Result<(), IntegerIssue> {
    if input.len() > MAXIMUM_INTEGER_TEXT_BYTES {
        Err(IntegerIssue::input_limit())
    } else {
        Ok(())
    }
}

fn check_output(bytes: usize) -> Result<(), IntegerIssue> {
    if bytes > MAXIMUM_INTEGER_TEXT_BYTES {
        Err(IntegerIssue::output_limit())
    } else {
        Ok(())
    }
}

fn check_value(value: &BigInt) -> Result<(), IntegerIssue> {
    if value.bits() > MAXIMUM_LIBRARY_INTEGER_BITS {
        Err(IntegerIssue::value_limit())
    } else {
        Ok(())
    }
}

fn invalid_backend_arguments() -> Diagnostic {
    Diagnostic::simple(
        "RUST_INTEGER_BACKEND_TYPE",
        "Rust integer backend received invalid arguments",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntegerIssue {
    code: &'static str,
    offset: Option<usize>,
    limit: Option<IntegerLimit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegerLimit {
    InputBytes,
    OutputBytes,
    ValueBits,
    Radix,
}

impl IntegerIssue {
    const fn invalid_syntax(offset: usize) -> Self {
        Self {
            code: "INTEGER_INVALID_SYNTAX",
            offset: Some(offset),
            limit: None,
        }
    }

    const fn invalid_radix() -> Self {
        Self {
            code: "INTEGER_INVALID_RADIX",
            offset: None,
            limit: Some(IntegerLimit::Radix),
        }
    }

    const fn input_limit() -> Self {
        Self {
            code: "INTEGER_INPUT_LIMIT",
            offset: None,
            limit: Some(IntegerLimit::InputBytes),
        }
    }

    const fn output_limit() -> Self {
        Self {
            code: "INTEGER_OUTPUT_LIMIT",
            offset: None,
            limit: Some(IntegerLimit::OutputBytes),
        }
    }

    const fn value_limit() -> Self {
        Self {
            code: "INTEGER_VALUE_LIMIT",
            offset: None,
            limit: Some(IntegerLimit::ValueBits),
        }
    }

    fn into_value(self) -> LibraryValue {
        let mut fields = BTreeMap::from([(
            LibraryKey::String("code".to_owned()),
            LibraryValue::String(self.code.to_owned()),
        )]);
        if let Some(offset) = self.offset {
            insert_integer(&mut fields, "offset", offset);
        }
        match self.limit {
            Some(IntegerLimit::InputBytes | IntegerLimit::OutputBytes) => {
                insert_integer(&mut fields, "maximum", MAXIMUM_INTEGER_TEXT_BYTES);
            }
            Some(IntegerLimit::ValueBits) => {
                fields.insert(
                    LibraryKey::String("maximumBits".to_owned()),
                    LibraryValue::Int(MAXIMUM_LIBRARY_INTEGER_BITS.into()),
                );
            }
            Some(IntegerLimit::Radix) => {
                fields.insert(
                    LibraryKey::String("minimum".to_owned()),
                    LibraryValue::Int(MINIMUM_INTEGER_RADIX.into()),
                );
                fields.insert(
                    LibraryKey::String("maximum".to_owned()),
                    LibraryValue::Int(MAXIMUM_INTEGER_RADIX.into()),
                );
            }
            None => {}
        }
        LibraryValue::Map(fields)
    }
}

fn insert_integer(fields: &mut BTreeMap<LibraryKey, LibraryValue>, name: &str, value: usize) {
    fields.insert(
        LibraryKey::String(name.to_owned()),
        LibraryValue::Int(value.into()),
    );
}
