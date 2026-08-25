#![forbid(unsafe_code)]

use num_bigint::BigInt;
use num_traits::Zero;

use super::{
    DecimalIssue, MAXIMUM_DECIMAL_INPUT_BYTES, MAXIMUM_DECIMAL_INTEGER_BITS,
    MAXIMUM_DECIMAL_INTEGER_DIGITS, MAXIMUM_DECIMAL_OUTPUT_BYTES, checked_scale,
};

pub(super) fn parse_scaled(input: &str, scale: &BigInt) -> Result<BigInt, DecimalIssue> {
    let scale = checked_scale(scale)?;
    if input.len() > MAXIMUM_DECIMAL_INPUT_BYTES {
        return Err(DecimalIssue::limit(
            "DECIMAL_INPUT_LIMIT",
            MAXIMUM_DECIMAL_INPUT_BYTES as u64,
        ));
    }
    let bytes = input.as_bytes();
    let mut offset = usize::from(bytes.first() == Some(&b'-'));
    let negative = offset == 1;
    let integer_start = offset;

    match bytes.get(offset).copied() {
        Some(b'0') => {
            offset += 1;
            if matches!(bytes.get(offset), Some(b'0'..=b'9')) {
                return Err(DecimalIssue::at("DECIMAL_SYNTAX", offset));
            }
        }
        Some(b'1'..=b'9') => {
            offset += 1;
            while matches!(bytes.get(offset), Some(b'0'..=b'9')) {
                offset += 1;
            }
        }
        _ => return Err(DecimalIssue::at("DECIMAL_SYNTAX", offset)),
    }
    let integer_end = offset;

    let (fraction_start, fraction_end) = if bytes.get(offset) == Some(&b'.') {
        offset += 1;
        let start = offset;
        while matches!(bytes.get(offset), Some(b'0'..=b'9')) {
            offset += 1;
        }
        if start == offset {
            return Err(DecimalIssue::at("DECIMAL_SYNTAX", offset));
        }
        (start, offset)
    } else {
        (offset, offset)
    };
    if offset != bytes.len() {
        return Err(DecimalIssue::at("DECIMAL_SYNTAX", offset));
    }

    let fraction_len = fraction_end.saturating_sub(fraction_start);
    if fraction_len > scale {
        let discarded_start = fraction_start.saturating_add(scale);
        if let Some(index) = bytes[discarded_start..fraction_end]
            .iter()
            .position(|byte| *byte != b'0')
        {
            return Err(DecimalIssue::at(
                "DECIMAL_PRECISION_LOSS",
                discarded_start.saturating_add(index),
            ));
        }
    }

    let integer_len = integer_end.saturating_sub(integer_start);
    let result_digits = integer_len.saturating_add(scale);
    if result_digits > MAXIMUM_DECIMAL_INTEGER_DIGITS {
        return Err(DecimalIssue::limit(
            "DECIMAL_INTEGER_LIMIT",
            MAXIMUM_DECIMAL_INTEGER_BITS,
        ));
    }

    let kept_fraction = fraction_len.min(scale);
    let mut digits = String::with_capacity(result_digits);
    digits.push_str(&input[integer_start..integer_end]);
    digits.push_str(&input[fraction_start..fraction_start.saturating_add(kept_fraction)]);
    digits.extend(std::iter::repeat_n(
        '0',
        scale.saturating_sub(kept_fraction),
    ));
    let mut value = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| DecimalIssue::simple("DECIMAL_SYNTAX"))?;
    if value.bits() > MAXIMUM_DECIMAL_INTEGER_BITS {
        return Err(DecimalIssue::limit(
            "DECIMAL_INTEGER_LIMIT",
            MAXIMUM_DECIMAL_INTEGER_BITS,
        ));
    }
    if negative && !value.is_zero() {
        value = -value;
    }
    Ok(value)
}

pub(super) fn format_scaled(value: &BigInt, scale: &BigInt) -> Result<String, DecimalIssue> {
    let scale = checked_scale(scale)?;
    if value.bits() > MAXIMUM_DECIMAL_INTEGER_BITS {
        return Err(DecimalIssue::limit(
            "DECIMAL_INTEGER_LIMIT",
            MAXIMUM_DECIMAL_INTEGER_BITS,
        ));
    }
    let encoded = value.to_string();
    let (negative, digits) = encoded
        .strip_prefix('-')
        .map_or((false, encoded.as_str()), |digits| (true, digits));
    let output_len = usize::from(negative).saturating_add(if scale == 0 {
        digits.len()
    } else if digits.len() > scale {
        digits.len().saturating_add(1)
    } else {
        scale.saturating_add(2)
    });
    if output_len > MAXIMUM_DECIMAL_OUTPUT_BYTES {
        return Err(DecimalIssue::limit(
            "DECIMAL_OUTPUT_LIMIT",
            MAXIMUM_DECIMAL_OUTPUT_BYTES as u64,
        ));
    }

    let mut output = String::with_capacity(output_len);
    if negative {
        output.push('-');
    }
    if scale == 0 {
        output.push_str(digits);
    } else if digits.len() > scale {
        let point = digits.len().saturating_sub(scale);
        output.push_str(&digits[..point]);
        output.push('.');
        output.push_str(&digits[point..]);
    } else {
        output.push_str("0.");
        output.extend(std::iter::repeat_n('0', scale.saturating_sub(digits.len())));
        output.push_str(digits);
    }
    Ok(output)
}
