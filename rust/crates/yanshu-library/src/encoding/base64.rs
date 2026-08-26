#![forbid(unsafe_code)]

use super::{EncodingIssue, check_input, check_output};

pub(super) fn encode_base64_text(input: &str) -> Result<String, EncodingIssue> {
    check_input(input)?;
    let output_len = input
        .len()
        .checked_add(2)
        .map(|value| value / 3)
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(EncodingIssue::output_limit)?;
    check_output(output_len)?;

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(output_len);
    for chunk in input.as_bytes().chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(char::from(ALPHABET[usize::from(first >> 2)]));
        output.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            output.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        } else {
            output.push('=');
        }
    }
    Ok(output)
}

pub(super) fn decode_base64_text(input: &str) -> Result<String, EncodingIssue> {
    check_input(input)?;
    if !input.len().is_multiple_of(4) {
        return Err(EncodingIssue::invalid_base64(input.len()));
    }
    let padding = input
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return Err(EncodingIssue::invalid_base64(
            input.len().saturating_sub(padding),
        ));
    }
    let output_len = input
        .len()
        .checked_div(4)
        .and_then(|value| value.checked_mul(3))
        .and_then(|value| value.checked_sub(padding))
        .ok_or_else(|| EncodingIssue::invalid_base64(input.len()))?;
    check_output(output_len)?;

    let mut output = Vec::with_capacity(output_len);
    let chunk_count = input.len() / 4;
    for (chunk_index, chunk) in input.as_bytes().chunks_exact(4).enumerate() {
        let offset = chunk_index.saturating_mul(4);
        let last = chunk_index + 1 == chunk_count;
        let first = decode_digit(chunk[0], offset)?;
        let second = decode_digit(chunk[1], offset + 1)?;
        let third = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || second & 0x0f != 0 {
                return Err(EncodingIssue::invalid_base64(offset + 2));
            }
            None
        } else {
            Some(decode_digit(chunk[2], offset + 2)?)
        };
        let fourth = if chunk[3] == b'=' {
            if !last || third.is_some_and(|value| value & 0x03 != 0) {
                return Err(EncodingIssue::invalid_base64(offset + 3));
            }
            None
        } else {
            if third.is_none() {
                return Err(EncodingIssue::invalid_base64(offset + 3));
            }
            Some(decode_digit(chunk[3], offset + 3)?)
        };

        output.push((first << 2) | (second >> 4));
        if let Some(third) = third {
            output.push(((second & 0x0f) << 4) | (third >> 2));
            if let Some(fourth) = fourth {
                output.push(((third & 0x03) << 6) | fourth);
            }
        }
    }
    debug_assert_eq!(output.len(), output_len);
    String::from_utf8(output)
        .map_err(|error| EncodingIssue::invalid_utf8(error.utf8_error().valid_up_to()))
}

fn decode_digit(byte: u8, offset: usize) -> Result<u8, EncodingIssue> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(EncodingIssue::invalid_base64(offset)),
    }
}
