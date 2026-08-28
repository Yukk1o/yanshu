#![forbid(unsafe_code)]

use num_bigint::BigInt;

use super::{IntegerIssue, check_output, check_value};

pub(super) fn format_radix(
    value: &BigInt,
    radix: u32,
) -> Result<crate::LibraryValue, IntegerIssue> {
    check_value(value)?;
    let output = value.to_str_radix(radix);
    check_output(output.len())?;
    Ok(crate::LibraryValue::String(output))
}
