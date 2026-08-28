#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use num_bigint::BigInt;

use super::{MAXIMUM_INTEGER_TEXT_BYTES, MAXIMUM_SIGNIFICANT_DIGITS, RustIntegerBackend};
use crate::{
    LibraryBackend, LibraryKey, LibraryRegistry, LibraryValue, MAXIMUM_LIBRARY_INTEGER_BITS,
    trusted_contract,
};

fn invoke(operation: &str, arguments: Vec<LibraryValue>) -> LibraryValue {
    RustIntegerBackend
        .invoke(operation, &arguments)
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
}

fn ok_integer(value: LibraryValue) -> BigInt {
    let LibraryValue::Ok(value) = value else {
        panic!("expected Ok")
    };
    let LibraryValue::Int(value) = *value else {
        panic!("expected Int")
    };
    value
}

fn ok_text(value: LibraryValue) -> String {
    let LibraryValue::Ok(value) = value else {
        panic!("expected Ok")
    };
    let LibraryValue::String(value) = *value else {
        panic!("expected String")
    };
    value
}

fn error_fields(value: LibraryValue) -> BTreeMap<LibraryKey, LibraryValue> {
    let LibraryValue::Err(value) = value else {
        panic!("expected Err")
    };
    let LibraryValue::Map(fields) = *value else {
        panic!("expected error Map")
    };
    fields
}

fn error_code(value: LibraryValue) -> String {
    let fields = error_fields(value);
    match fields.get(&LibraryKey::String("code".to_owned())) {
        Some(LibraryValue::String(value)) => value.clone(),
        _ => panic!("expected error code"),
    }
}

fn error_offset(value: LibraryValue) -> (String, usize) {
    let fields = error_fields(value);
    let code = match fields.get(&LibraryKey::String("code".to_owned())) {
        Some(LibraryValue::String(value)) => value.clone(),
        _ => panic!("expected error code"),
    };
    let offset = match fields.get(&LibraryKey::String("offset".to_owned())) {
        Some(LibraryValue::Int(value)) => {
            usize::try_from(value).unwrap_or_else(|_| panic!("offset must fit usize"))
        }
        _ => panic!("expected integer offset"),
    };
    (code, offset)
}

fn parse_decimal(input: &str) -> LibraryValue {
    invoke(
        "parse-decimal",
        vec![LibraryValue::String(input.to_owned())],
    )
}

fn parse_radix(input: &str, radix: i64) -> LibraryValue {
    invoke(
        "parse-radix",
        vec![
            LibraryValue::String(input.to_owned()),
            LibraryValue::Int(radix.into()),
        ],
    )
}

fn format_radix(value: BigInt, radix: i64) -> LibraryValue {
    invoke(
        "format-radix",
        vec![LibraryValue::Int(value), LibraryValue::Int(radix.into())],
    )
}

#[test]
fn decimal_parsing_is_strict_and_normalizes_zeroes() {
    for (input, expected) in [
        ("0", 0_i64),
        ("-0", 0),
        ("00042", 42),
        ("-0012", -12),
        ("9007199254740993", 9_007_199_254_740_993),
    ] {
        assert_eq!(ok_integer(parse_decimal(input)), BigInt::from(expected));
    }
}

#[test]
fn radix_parsing_and_formatting_cover_the_portable_range() {
    for (input, radix, expected) in [
        ("101", 2, 5_i64),
        ("ff", 16, 255),
        ("FF", 16, 255),
        ("z", 36, 35),
    ] {
        assert_eq!(
            ok_integer(parse_radix(input, radix)),
            BigInt::from(expected)
        );
    }
    for (value, radix, expected) in [
        (0_i64, 2, "0"),
        (255, 16, "ff"),
        (-5, 2, "-101"),
        (35, 36, "z"),
    ] {
        assert_eq!(ok_text(format_radix(value.into(), radix)), expected);
    }
}

#[test]
fn syntax_errors_identify_the_first_invalid_byte() {
    for (input, radix, offset) in [
        ("", 10, 0),
        ("-", 10, 1),
        ("+1", 10, 0),
        (" 1", 10, 0),
        ("1_0", 10, 1),
        ("0x10", 16, 1),
        ("2", 2, 0),
        ("一", 10, 0),
    ] {
        assert_eq!(
            error_offset(parse_radix(input, radix)),
            ("INTEGER_INVALID_SYNTAX".to_owned(), offset)
        );
    }
}

#[test]
fn invalid_radix_reports_the_supported_closed_interval() {
    for radix in [-2_i64, 0, 1, 37, i64::MAX] {
        let fields = error_fields(parse_radix("10", radix));
        assert_eq!(
            fields.get(&LibraryKey::String("code".to_owned())),
            Some(&LibraryValue::String("INTEGER_INVALID_RADIX".to_owned()))
        );
        assert_eq!(
            fields.get(&LibraryKey::String("minimum".to_owned())),
            Some(&LibraryValue::Int(2.into()))
        );
        assert_eq!(
            fields.get(&LibraryKey::String("maximum".to_owned())),
            Some(&LibraryValue::Int(36.into()))
        );
    }
}

#[test]
fn input_and_value_limits_are_enforced_before_unbounded_parsing() {
    assert_eq!(
        error_code(parse_decimal(&"0".repeat(MAXIMUM_INTEGER_TEXT_BYTES + 1))),
        "INTEGER_INPUT_LIMIT"
    );
    assert_eq!(
        ok_integer(parse_radix(&"1".repeat(65_536), 2)).bits(),
        MAXIMUM_LIBRARY_INTEGER_BITS
    );
    assert_eq!(
        error_code(parse_radix(&"1".repeat(65_537), 2)),
        "INTEGER_VALUE_LIMIT"
    );
    let too_large = BigInt::from(1_u8) << MAXIMUM_LIBRARY_INTEGER_BITS;
    assert_eq!(
        error_code(format_radix(too_large, 10)),
        "INTEGER_VALUE_LIMIT"
    );
}

#[test]
fn significant_digit_table_is_a_conservative_power_boundary() {
    let portable_limit = BigInt::from(1_u8) << MAXIMUM_LIBRARY_INTEGER_BITS;
    for (index, digits) in MAXIMUM_SIGNIFICANT_DIGITS.iter().copied().enumerate() {
        let radix = u32::try_from(index).unwrap_or_else(|_| unreachable!()) + 2;
        let exponent = u32::try_from(digits).unwrap_or_else(|_| unreachable!());
        assert!(BigInt::from(radix).pow(exponent) >= portable_limit);
        assert!(BigInt::from(radix).pow(exponent - 1) < portable_limit);
    }
}

#[test]
fn fuel_tracks_text_and_integer_magnitude_even_for_invalid_input() {
    let registry = LibraryRegistry::rust_standard();
    let small = registry
        .call_fuel(
            "integer",
            1,
            "parse-decimal",
            &[LibraryValue::String("1".to_owned())],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let large = registry
        .call_fuel(
            "integer",
            1,
            "parse-decimal",
            &[LibraryValue::String("1".repeat(4096))],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let invalid = registry
        .call_fuel(
            "integer",
            1,
            "parse-decimal",
            &[LibraryValue::String("!".repeat(4096))],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let large_format = registry
        .call_fuel(
            "integer",
            1,
            "format-radix",
            &[
                LibraryValue::Int(BigInt::from(1_u8) << 4095_u32),
                LibraryValue::Int(16.into()),
            ],
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(large > small);
    assert!(invalid > small);
    assert!(large_format > small);
    assert!(trusted_contract("integer", 1).is_some());
    assert!(trusted_contract("integer", 2).is_none());
}
