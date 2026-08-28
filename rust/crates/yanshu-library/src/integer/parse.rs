#![forbid(unsafe_code)]

use num_bigint::BigInt;

use super::{IntegerIssue, check_input, check_value, maximum_significant_digits};

pub(super) fn parse_decimal(input: &str) -> Result<crate::LibraryValue, IntegerIssue> {
    parse_integer(input, 10).map(crate::LibraryValue::Int)
}

pub(super) fn parse_radix(input: &str, radix: u32) -> Result<crate::LibraryValue, IntegerIssue> {
    parse_integer(input, radix).map(crate::LibraryValue::Int)
}

fn parse_integer(input: &str, radix: u32) -> Result<BigInt, IntegerIssue> {
    check_input(input)?;
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Err(IntegerIssue::invalid_syntax(0));
    }
    let (negative, digits_start) = if bytes[0] == b'-' {
        (true, 1)
    } else {
        (false, 0)
    };
    if digits_start == bytes.len() {
        return Err(IntegerIssue::invalid_syntax(digits_start));
    }

    let mut first_significant = bytes.len();
    for (index, byte) in bytes[digits_start..].iter().copied().enumerate() {
        let offset = digits_start.saturating_add(index);
        let digit = digit_value(byte)
            .filter(|digit| *digit < radix)
            .ok_or_else(|| IntegerIssue::invalid_syntax(offset))?;
        if digit != 0 && first_significant == bytes.len() {
            first_significant = offset;
        }
    }

    let significant_digits = bytes.len().saturating_sub(first_significant);
    if significant_digits > maximum_significant_digits(radix) {
        return Err(IntegerIssue::value_limit());
    }
    if significant_digits == 0 {
        return Ok(BigInt::ZERO);
    }

    let magnitude = BigInt::parse_bytes(&bytes[digits_start..], radix)
        .ok_or_else(|| IntegerIssue::invalid_syntax(digits_start))?;
    let value = if negative { -magnitude } else { magnitude };
    check_value(&value)?;
    Ok(value)
}

fn digit_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(byte - b'A') + 10),
        _ => None,
    }
}
