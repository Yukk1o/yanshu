#![forbid(unsafe_code)]

use super::{EncodingIssue, check_input, check_output};

pub(super) fn encode_hex_text(input: &str) -> Result<String, EncodingIssue> {
    check_input(input)?;
    let output_len = input
        .len()
        .checked_mul(2)
        .ok_or_else(EncodingIssue::output_limit)?;
    check_output(output_len)?;
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(output_len);
    for byte in input.bytes() {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

pub(super) fn decode_hex_text(input: &str) -> Result<String, EncodingIssue> {
    check_input(input)?;
    if !input.len().is_multiple_of(2) {
        return Err(EncodingIssue::invalid_hex(input.len()));
    }
    let output_len = input.len() / 2;
    check_output(output_len)?;
    let mut output = Vec::with_capacity(output_len);
    for (index, pair) in input.as_bytes().chunks_exact(2).enumerate() {
        let offset = index.saturating_mul(2);
        let high = decode_digit(pair[0]).ok_or_else(|| EncodingIssue::invalid_hex(offset))?;
        let low = decode_digit(pair[1]).ok_or_else(|| EncodingIssue::invalid_hex(offset + 1))?;
        output.push((high << 4) | low);
    }
    String::from_utf8(output)
        .map_err(|error| EncodingIssue::invalid_utf8(error.utf8_error().valid_up_to()))
}

fn decode_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
