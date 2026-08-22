use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

pub const MAXIMUM_MCP_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAXIMUM_MCP_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

pub(crate) struct ReadFailure {
    pub(crate) diagnostic: Diagnostic,
    pub(crate) recoverable: bool,
}

pub(crate) fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, ReadFailure> {
    let mut line = Vec::new();
    let maximum = u64::try_from(MAXIMUM_MCP_INPUT_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    reader
        .take(maximum)
        .read_until(b'\n', &mut line)
        .map_err(|error| ReadFailure {
            diagnostic: Diagnostic::new(
                "MCP_INPUT_READ",
                "MCP server could not read a stdio message",
                json!({ "kind": error.kind().to_string() }),
            ),
            recoverable: false,
        })?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > MAXIMUM_MCP_INPUT_BYTES {
        return Err(ReadFailure {
            diagnostic: Diagnostic::new(
                "MCP_MESSAGE_LIMIT",
                "MCP input message exceeds the configured byte limit",
                json!({
                    "actualAtLeast": line.len(),
                    "maximum": MAXIMUM_MCP_INPUT_BYTES,
                }),
            ),
            recoverable: false,
        });
    }
    if !line.ends_with(b"\n") {
        return Err(ReadFailure {
            diagnostic: Diagnostic::simple(
                "MCP_MESSAGE_TERMINATOR",
                "MCP stdio message must end with a newline",
            ),
            recoverable: false,
        });
    }
    let _newline = line.pop();
    if line.ends_with(b"\r") {
        let _carriage_return = line.pop();
    }
    if line.is_empty() {
        return Err(ReadFailure {
            diagnostic: Diagnostic::simple(
                "MCP_MESSAGE_JSON",
                "MCP stdio message must contain one JSON-RPC object",
            ),
            recoverable: true,
        });
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|_| ReadFailure {
            diagnostic: Diagnostic::simple(
                "MCP_MESSAGE_JSON",
                "MCP stdio message is not one valid JSON document",
            ),
            recoverable: true,
        })
}

pub(crate) fn write_message<W: Write>(writer: &mut W, message: &Value) -> YanshuResult<()> {
    let body = serde_json::to_vec(message).map_err(|_| {
        Diagnostic::simple(
            "MCP_OUTPUT_JSON",
            "MCP server could not encode its response",
        )
    })?;
    if body.len() > MAXIMUM_MCP_OUTPUT_BYTES {
        return Err(Diagnostic::new(
            "MCP_OUTPUT_LIMIT",
            "MCP response exceeds the configured byte limit",
            json!({ "actual": body.len(), "maximum": MAXIMUM_MCP_OUTPUT_BYTES }),
        ));
    }
    writer
        .write_all(&body)
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| {
            Diagnostic::new(
                "MCP_OUTPUT_WRITE",
                "MCP server could not write its response",
                json!({ "kind": error.kind().to_string() }),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use serde_json::json;

    use super::{MAXIMUM_MCP_INPUT_BYTES, read_message, write_message};

    #[test]
    fn reads_and_writes_newline_delimited_json() {
        let mut reader = BufReader::new(Cursor::new(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n".to_vec(),
        ));
        let message = read_message(&mut reader)
            .unwrap_or_else(|failure| panic!("{}", failure.diagnostic))
            .unwrap_or_else(|| panic!("message unexpectedly ended"));
        assert_eq!(message["method"], "ping");

        let mut output = Vec::new();
        write_message(
            &mut output,
            &json!({ "jsonrpc": "2.0", "id": 1, "result": {} }),
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(output.last(), Some(&b'\n'));
        let encoded: serde_json::Value = serde_json::from_slice(&output)
            .unwrap_or_else(|error| panic!("response is not JSON: {error}"));
        assert_eq!(encoded["id"], 1);
    }

    #[test]
    fn distinguishes_recoverable_json_from_fatal_framing_errors() {
        let invalid = read_message(&mut BufReader::new(Cursor::new(b"not-json\n".to_vec())))
            .err()
            .unwrap_or_else(|| panic!("invalid JSON unexpectedly passed"));
        assert!(invalid.recoverable);
        assert_eq!(invalid.diagnostic.code, "MCP_MESSAGE_JSON");

        let unterminated = read_message(&mut BufReader::new(Cursor::new(b"{}".to_vec())))
            .err()
            .unwrap_or_else(|| panic!("unterminated message unexpectedly passed"));
        assert!(!unterminated.recoverable);
        assert_eq!(unterminated.diagnostic.code, "MCP_MESSAGE_TERMINATOR");

        let oversized = vec![b'x'; MAXIMUM_MCP_INPUT_BYTES + 1];
        let failure = read_message(&mut BufReader::new(Cursor::new(oversized)))
            .err()
            .unwrap_or_else(|| panic!("oversized message unexpectedly passed"));
        assert!(!failure.recoverable);
        assert_eq!(failure.diagnostic.code, "MCP_MESSAGE_LIMIT");
    }
}
