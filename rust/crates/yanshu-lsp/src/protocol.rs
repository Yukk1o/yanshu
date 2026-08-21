use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

pub const MAXIMUM_LSP_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const MAXIMUM_HEADER_BYTES: usize = 16 * 1024;
const MAXIMUM_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAXIMUM_HEADER_COUNT: usize = 32;

pub fn read_message<R: BufRead>(reader: &mut R) -> YanshuResult<Option<Value>> {
    let mut content_length = None;
    let mut total_header_bytes = 0_usize;
    let mut header_count = 0_usize;

    loop {
        let line = read_bounded_header_line(reader)?;
        if line.is_empty() {
            if header_count == 0 {
                return Ok(None);
            }
            return Err(protocol_error(
                "LSP_HEADER_EOF",
                "LSP headers ended before the required blank line",
            ));
        }
        total_header_bytes = total_header_bytes.saturating_add(line.len());
        if total_header_bytes > MAXIMUM_HEADER_BYTES {
            return Err(protocol_error(
                "LSP_HEADER_LIMIT",
                "LSP headers exceed the configured byte limit",
            ));
        }
        if line == b"\r\n" {
            break;
        }
        header_count += 1;
        if header_count > MAXIMUM_HEADER_COUNT {
            return Err(protocol_error(
                "LSP_HEADER_LIMIT",
                "LSP message has too many headers",
            ));
        }
        if !line.is_ascii() {
            return Err(protocol_error(
                "LSP_HEADER_ENCODING",
                "LSP headers must use ASCII encoding",
            ));
        }
        let text = std::str::from_utf8(&line).map_err(|_| {
            protocol_error(
                "LSP_HEADER_ENCODING",
                "LSP headers must be ASCII-compatible",
            )
        })?;
        let header = text.trim_end_matches("\r\n");
        let Some((name, value)) = header.split_once(':') else {
            return Err(protocol_error(
                "LSP_HEADER_SHAPE",
                "LSP header must contain a name and value",
            ));
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(protocol_error(
                    "LSP_CONTENT_LENGTH",
                    "LSP message contains duplicate Content-Length headers",
                ));
            }
            let parsed = value.trim().parse::<usize>().map_err(|_| {
                protocol_error(
                    "LSP_CONTENT_LENGTH",
                    "LSP Content-Length must be a decimal byte count",
                )
            })?;
            if parsed == 0 || parsed > MAXIMUM_LSP_MESSAGE_BYTES {
                return Err(Diagnostic::new(
                    "LSP_MESSAGE_LIMIT",
                    "LSP message body is outside the configured byte limit",
                    json!({ "actual": parsed, "maximum": MAXIMUM_LSP_MESSAGE_BYTES }),
                ));
            }
            content_length = Some(parsed);
        }
    }

    let length = content_length.ok_or_else(|| {
        protocol_error(
            "LSP_CONTENT_LENGTH",
            "LSP message is missing Content-Length",
        )
    })?;
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).map_err(|error| {
        Diagnostic::new(
            "LSP_BODY_READ",
            "LSP message body ended before Content-Length bytes were read",
            json!({ "kind": error.kind().to_string() }),
        )
    })?;
    serde_json::from_slice(&body).map(Some).map_err(|_| {
        protocol_error(
            "LSP_BODY_JSON",
            "LSP message body is not one valid JSON document",
        )
    })
}

pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> YanshuResult<()> {
    let body = serde_json::to_vec(message).map_err(|_| {
        protocol_error(
            "LSP_OUTPUT_JSON",
            "LSP server could not encode its response",
        )
    })?;
    if body.len() > MAXIMUM_LSP_MESSAGE_BYTES {
        return Err(Diagnostic::new(
            "LSP_MESSAGE_LIMIT",
            "LSP response exceeds the configured byte limit",
            json!({ "actual": body.len(), "maximum": MAXIMUM_LSP_MESSAGE_BYTES }),
        ));
    }
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .and_then(|()| writer.write_all(&body))
        .and_then(|()| writer.flush())
        .map_err(|error| {
            Diagnostic::new(
                "LSP_OUTPUT_WRITE",
                "LSP server could not write its response",
                json!({ "kind": error.kind().to_string() }),
            )
        })
}

fn read_bounded_header_line<R: BufRead>(reader: &mut R) -> YanshuResult<Vec<u8>> {
    let mut line = Vec::new();
    let maximum = u64::try_from(MAXIMUM_HEADER_LINE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    reader
        .take(maximum)
        .read_until(b'\n', &mut line)
        .map_err(|error| {
            Diagnostic::new(
                "LSP_HEADER_READ",
                "LSP server could not read a message header",
                json!({ "kind": error.kind().to_string() }),
            )
        })?;
    if line.len() > MAXIMUM_HEADER_LINE_BYTES || (!line.is_empty() && !line.ends_with(b"\n")) {
        return Err(protocol_error(
            "LSP_HEADER_LIMIT",
            "LSP header line exceeds the configured byte limit",
        ));
    }
    Ok(line)
}

fn protocol_error(code: &'static str, message: &'static str) -> Diagnostic {
    Diagnostic::simple(code, message)
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use serde_json::json;

    use super::{MAXIMUM_HEADER_LINE_BYTES, read_message, write_message};

    #[test]
    fn reads_and_writes_content_length_framed_json() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let input = [
            format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes(),
            body.to_vec(),
        ]
        .concat();
        let mut reader = BufReader::new(Cursor::new(input));
        let message = read_message(&mut reader)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("message unexpectedly ended"));
        assert_eq!(message["method"], "initialize");

        let mut output = Vec::new();
        write_message(
            &mut output,
            &json!({ "jsonrpc": "2.0", "id": 1, "result": null }),
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert!(output.starts_with(b"Content-Length: "));
        assert!(output.ends_with(br#"{"id":1,"jsonrpc":"2.0","result":null}"#));
    }

    #[test]
    fn rejects_duplicate_lengths_and_unbounded_headers() {
        let duplicate = b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        let error = read_message(&mut BufReader::new(Cursor::new(duplicate)))
            .err()
            .unwrap_or_else(|| panic!("duplicate length unexpectedly passed"));
        assert_eq!(error.code, "LSP_CONTENT_LENGTH");

        let oversized = vec![b'x'; MAXIMUM_HEADER_LINE_BYTES + 1];
        let error = read_message(&mut BufReader::new(Cursor::new(oversized)))
            .err()
            .unwrap_or_else(|| panic!("oversized header unexpectedly passed"));
        assert_eq!(error.code, "LSP_HEADER_LIMIT");
    }
}
