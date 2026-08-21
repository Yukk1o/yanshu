#![forbid(unsafe_code)]

mod document;
mod protocol;
mod server;

use std::io::{BufRead, Write};

use yanshu_diagnostic::YanshuResult;

pub use protocol::{MAXIMUM_LSP_MESSAGE_BYTES, read_message, write_message};
pub use server::LanguageServer;

pub fn run_stdio<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> YanshuResult<()> {
    let mut server = LanguageServer::new();
    while let Some(message) = read_message(reader)? {
        for output in server.handle_message(&message) {
            write_message(writer, &output)?;
        }
        if server.should_exit() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use serde_json::{Value, json};

    use super::{read_message, run_stdio};

    fn framed(message: &Value) -> Vec<u8> {
        let body = serde_json::to_vec(message)
            .unwrap_or_else(|error| panic!("test message failed to encode: {error}"));
        [
            format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes(),
            body,
        ]
        .concat()
    }

    #[test]
    fn stdio_loop_completes_initialize_shutdown_and_exit() {
        let input = [
            framed(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
            })),
            framed(&json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" })),
            framed(&json!({ "jsonrpc": "2.0", "method": "exit" })),
        ]
        .concat();
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();
        run_stdio(&mut reader, &mut output).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

        let mut responses = BufReader::new(Cursor::new(output));
        let initialize = read_message(&mut responses)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("initialize response missing"));
        let shutdown = read_message(&mut responses)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("shutdown response missing"));
        let end = read_message(&mut responses).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(initialize["id"], 1);
        assert_eq!(shutdown["id"], 2);
        assert_eq!(end, None);
    }
}
