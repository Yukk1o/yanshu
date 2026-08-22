#![forbid(unsafe_code)]

mod protocol;
mod server;
mod tools;

use std::io::{BufRead, Write};

use yanshu_diagnostic::YanshuResult;

pub use protocol::{MAXIMUM_MCP_INPUT_BYTES, MAXIMUM_MCP_OUTPUT_BYTES};
pub use server::{LATEST_LEGACY_PROTOCOL_VERSION, MODERN_PROTOCOL_VERSION, McpServer};

pub fn run_stdio<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> YanshuResult<()> {
    let mut server = McpServer::new();
    loop {
        let message = match protocol::read_message(reader) {
            Ok(Some(message)) => message,
            Ok(None) => return Ok(()),
            Err(failure) => {
                protocol::write_message(
                    writer,
                    &server::parse_error_response(&failure.diagnostic),
                )?;
                if failure.recoverable {
                    continue;
                }
                return Err(failure.diagnostic);
            }
        };
        if let Some(response) = server.handle_message(&message) {
            protocol::write_message(writer, &response)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use serde_json::{Value, json};

    use crate::{LATEST_LEGACY_PROTOCOL_VERSION, run_stdio};

    fn line(message: &Value) -> Vec<u8> {
        let mut encoded = serde_json::to_vec(message)
            .unwrap_or_else(|error| panic!("test message failed to encode: {error}"));
        encoded.push(b'\n');
        encoded
    }

    #[test]
    fn stdio_loop_handles_client_sequence_without_non_json_stdout() {
        let input = [
            line(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": LATEST_LEGACY_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1" }
                }
            })),
            line(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
            line(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })),
        ]
        .concat();
        let mut output = Vec::new();
        run_stdio(&mut BufReader::new(Cursor::new(input)), &mut output)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

        let lines = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let response: Value = serde_json::from_slice(line)
                .unwrap_or_else(|error| panic!("stdout line is not JSON: {error}"));
            assert_eq!(response["jsonrpc"], "2.0");
        }
    }

    #[test]
    fn recoverable_parse_error_does_not_desynchronize_next_message() {
        let input = [
            b"not-json\n".to_vec(),
            line(&json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })),
        ]
        .concat();
        let mut output = Vec::new();
        run_stdio(&mut BufReader::new(Cursor::new(input)), &mut output)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let responses = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                serde_json::from_slice::<Value>(line)
                    .unwrap_or_else(|error| panic!("stdout line is not JSON: {error}"))
            })
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["id"], 1);
    }
}
