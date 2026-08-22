use serde_json::{Map, Value, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::tools::{self, MAXIMUM_TOOL_PAYLOAD_BYTES};

pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LATEST_LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const SERVER_NAME: &str = "yanshu-mcp";
const SERVER_INSTRUCTIONS: &str = "Read-only Yanshu source tools. Pass the current complete .yan source text. The server never reads or writes files, executes guest code, calls providers, or accesses the network. formattedSource is a candidate; review text is generated, non-executable, and never canonical input.";
const RESPONSE_FIXED_OVERHEAD_BYTES: usize = 1024 * 1024;
const REQUEST_FIXED_OVERHEAD_BYTES: usize = 64 * 1024;

// structuredContent serializes once and its compatibility TextContent copy can
// at most double the already-serialized JSON through quote/backslash escaping.
const _: () = assert!(
    MAXIMUM_TOOL_PAYLOAD_BYTES * 3 + RESPONSE_FIXED_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_MCP_OUTPUT_BYTES
);
const _: () = assert!(
    tools::MAXIMUM_TOOL_SOURCE_BYTES * 6 + REQUEST_FIXED_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_MCP_INPUT_BYTES
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolEra {
    Legacy,
    Modern,
}

struct RpcError {
    code: i64,
    message: &'static str,
    data: Value,
}

pub struct McpServer {
    legacy_protocol: Option<&'static str>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            legacy_protocol: None,
        }
    }

    #[must_use]
    pub fn handle_message(&mut self, message: &Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error_response(
                Value::Null,
                invalid_request("MCP message must be one JSON-RPC object"),
            ));
        };
        let id = match request_id(object.get("id")) {
            Ok(id) => id,
            Err(error) => return Some(error_response(Value::Null, error)),
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return id.map(|id| error_response(id, invalid_request("jsonrpc must equal 2.0")));
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return Some(error_response(
                id.unwrap_or(Value::Null),
                invalid_request("MCP client messages must contain a method"),
            ));
        };
        let Some(id) = id else {
            self.handle_notification(method);
            return None;
        };
        Some(match self.handle_request(method, object.get("params")) {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, error),
        })
    }

    fn handle_notification(&mut self, _method: &str) {
        // notifications/initialized and notifications/cancelled carry no work
        // for this sequential, stateless, read-only server. Unknown JSON-RPC
        // notifications also receive no response as required by JSON-RPC.
    }

    fn handle_request(&mut self, method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
        match method {
            "initialize" => self.initialize(params),
            "server/discover" => self.discover(params),
            "ping" => self.ping(params),
            "tools/list" => self.list_tools(params),
            "tools/call" => self.call_tool(params),
            _ => Err(RpcError {
                code: -32601,
                message: "MCP method was not found",
                data: json!({ "yanshuCode": "MCP_METHOD_NOT_FOUND" }),
            }),
        }
    }

    fn initialize(&mut self, params: Option<&Value>) -> Result<Value, RpcError> {
        let object = required_object(params, "initialize params must be an object")?;
        let requested = object
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("initialize requires protocolVersion"))?;
        if !object.get("capabilities").is_some_and(Value::is_object) {
            return Err(invalid_params("initialize requires a capabilities object"));
        }
        validate_client_info(
            object.get("clientInfo"),
            "initialize requires clientInfo with string name and version",
        )?;
        let selected = LEGACY_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|version| *version == requested)
            .unwrap_or(LATEST_LEGACY_PROTOCOL_VERSION);
        self.legacy_protocol = Some(selected);
        Ok(json!({
            "protocolVersion": selected,
            "capabilities": { "tools": {} },
            "serverInfo": server_info(),
            "instructions": SERVER_INSTRUCTIONS,
        }))
    }

    fn discover(&self, params: Option<&Value>) -> Result<Value, RpcError> {
        validate_modern_metadata(params)?;
        Ok(json!({
            "resultType": "complete",
            "supportedVersions": [MODERN_PROTOCOL_VERSION],
            "capabilities": { "tools": {} },
            "instructions": SERVER_INSTRUCTIONS,
            "ttlMs": 3_600_000,
            "cacheScope": "public",
            "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
        }))
    }

    fn ping(&self, params: Option<&Value>) -> Result<Value, RpcError> {
        if contains_modern_version(params) {
            validate_modern_metadata(params)?;
            Ok(modern_result(json!({})))
        } else {
            required_or_empty_object(params)?;
            Ok(json!({}))
        }
    }

    fn list_tools(&self, params: Option<&Value>) -> Result<Value, RpcError> {
        let era = self.request_era(params)?;
        if required_or_empty_object(params)?
            .get("cursor")
            .is_some_and(|cursor| !cursor.is_null())
        {
            return Err(invalid_params("this bounded tool list has no cursor"));
        }
        let tools = tools::definitions();
        Ok(match era {
            ProtocolEra::Legacy => json!({ "tools": tools }),
            ProtocolEra::Modern => json!({
                "resultType": "complete",
                "tools": tools,
                "ttlMs": 3_600_000,
                "cacheScope": "public",
                "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
            }),
        })
    }

    fn call_tool(&self, params: Option<&Value>) -> Result<Value, RpcError> {
        let era = self.request_era(params)?;
        let object = required_object(params, "tools/call params must be an object")?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("tools/call requires a string name"))?;
        if !is_known_tool(name) {
            return Err(RpcError {
                code: -32602,
                message: "MCP request named an unknown tool",
                data: json!({ "yanshuCode": "MCP_TOOL_UNKNOWN" }),
            });
        }
        let empty_arguments = Value::Object(Map::new());
        let arguments = object.get("arguments").unwrap_or(&empty_arguments);
        if !arguments.is_object() {
            return Ok(tool_diagnostic(
                Diagnostic::simple(
                    "MCP_TOOL_ARGUMENTS",
                    "tool arguments must be one JSON object",
                ),
                era,
            ));
        }
        match tools::call(name, arguments) {
            Ok(payload) => match tool_result(payload, false, era) {
                Ok(result) => Ok(result),
                Err(diagnostic) => Ok(tool_diagnostic(diagnostic, era)),
            },
            Err(diagnostic) => Ok(tool_diagnostic(diagnostic, era)),
        }
    }

    fn request_era(&self, params: Option<&Value>) -> Result<ProtocolEra, RpcError> {
        if contains_modern_version(params) {
            validate_modern_metadata(params)?;
            return Ok(ProtocolEra::Modern);
        }
        if self.legacy_protocol.is_some() {
            required_or_empty_object(params)?;
            return Ok(ProtocolEra::Legacy);
        }
        Err(RpcError {
            code: -32600,
            message: "legacy MCP client must initialize before using tools",
            data: json!({ "yanshuCode": "MCP_NOT_INITIALIZED" }),
        })
    }
}

fn tool_diagnostic(diagnostic: Diagnostic, era: ProtocolEra) -> Value {
    let payload = diagnostic.public_json();
    match tool_result(payload, true, era) {
        Ok(result) => result,
        Err(_) => {
            let fallback = json!({
                "ok": false,
                "error": {
                    "code": "MCP_TOOL_OUTPUT_LIMIT",
                    "message": "tool error response exceeded the configured byte limit",
                    "details": {}
                }
            });
            tool_result_unchecked(fallback, true, era)
        }
    }
}

fn tool_result(payload: Value, is_error: bool, era: ProtocolEra) -> YanshuResult<Value> {
    let text = serde_json::to_string(&payload).map_err(|_| {
        Diagnostic::simple(
            "MCP_TOOL_OUTPUT_JSON",
            "MCP tool could not encode its structured result",
        )
    })?;
    if text.len() > MAXIMUM_TOOL_PAYLOAD_BYTES {
        return Err(Diagnostic::new(
            "MCP_TOOL_OUTPUT_LIMIT",
            "MCP tool result exceeds the configured serialized byte limit",
            json!({
                "actual": text.len(),
                "maximum": MAXIMUM_TOOL_PAYLOAD_BYTES,
            }),
        ));
    }
    Ok(tool_result_with_text(payload, text, is_error, era))
}

fn tool_result_unchecked(payload: Value, is_error: bool, era: ProtocolEra) -> Value {
    let text = "{\"ok\":false,\"error\":{\"code\":\"MCP_TOOL_OUTPUT_LIMIT\",\"message\":\"tool error response exceeded the configured byte limit\",\"details\":{}}}".to_owned();
    tool_result_with_text(payload, text, is_error, era)
}

fn tool_result_with_text(payload: Value, text: String, is_error: bool, era: ProtocolEra) -> Value {
    let result = json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": payload,
        "isError": is_error,
    });
    match era {
        ProtocolEra::Legacy => result,
        ProtocolEra::Modern => modern_result(result),
    }
}

fn modern_result(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("resultType".to_owned(), json!("complete"));
        object.insert(
            "_meta".to_owned(),
            json!({ "io.modelcontextprotocol/serverInfo": server_info() }),
        );
    }
    value
}

fn contains_modern_version(params: Option<&Value>) -> bool {
    params
        .and_then(Value::as_object)
        .and_then(|object| object.get("_meta"))
        .and_then(Value::as_object)
        .is_some_and(|metadata| metadata.contains_key("io.modelcontextprotocol/protocolVersion"))
}

fn validate_modern_metadata(params: Option<&Value>) -> Result<(), RpcError> {
    let object = required_object(params, "modern MCP request params must be an object")?;
    let metadata = object
        .get("_meta")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_params("modern MCP request requires _meta"))?;
    let protocol_version = metadata
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("modern MCP request requires protocol version metadata"))?;
    if protocol_version != MODERN_PROTOCOL_VERSION {
        return Err(RpcError {
            code: -32022,
            message: "MCP protocol version is not supported",
            data: json!({
                "yanshuCode": "MCP_PROTOCOL_VERSION",
                "supported": [MODERN_PROTOCOL_VERSION],
                "requested": protocol_version,
            }),
        });
    }
    if !metadata
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err(invalid_params(
            "modern MCP request requires client capabilities metadata",
        ));
    }
    if let Some(client_info) = metadata.get("io.modelcontextprotocol/clientInfo") {
        validate_client_info(
            Some(client_info),
            "MCP clientInfo metadata requires string name and version",
        )?;
    }
    Ok(())
}

fn validate_client_info(value: Option<&Value>, message: &'static str) -> Result<(), RpcError> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_params(message))?;
    if !object.get("name").is_some_and(Value::is_string)
        || !object.get("version").is_some_and(Value::is_string)
    {
        return Err(invalid_params(message));
    }
    Ok(())
}

fn required_or_empty_object(params: Option<&Value>) -> Result<&Map<String, Value>, RpcError> {
    match params {
        Some(value) => value
            .as_object()
            .ok_or_else(|| invalid_params("MCP request params must be an object")),
        None => Ok(empty_object()),
    }
}

fn empty_object() -> &'static Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Map::new)
}

fn required_object<'value>(
    value: Option<&'value Value>,
    message: &'static str,
) -> Result<&'value Map<String, Value>, RpcError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_params(message))
}

fn request_id(value: Option<&Value>) -> Result<Option<Value>, RpcError> {
    match value {
        None => Ok(None),
        Some(Value::String(_) | Value::Number(_)) => Ok(value.cloned()),
        Some(_) => Err(invalid_request(
            "JSON-RPC request id must be a string or number",
        )),
    }
}

fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "yanshu.inspect_source" | "yanshu.format_source" | "yanshu.review_source"
    )
}

fn server_info() -> Value {
    json!({ "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, error: RpcError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": error.code,
            "message": error.message,
            "data": error.data,
        }
    })
}

fn invalid_request(message: &'static str) -> RpcError {
    RpcError {
        code: -32600,
        message,
        data: json!({ "yanshuCode": "MCP_INVALID_REQUEST" }),
    }
}

fn invalid_params(message: &'static str) -> RpcError {
    RpcError {
        code: -32602,
        message,
        data: json!({ "yanshuCode": "MCP_INVALID_PARAMS" }),
    }
}

pub(crate) fn parse_error_response(diagnostic: &Diagnostic) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32700,
            "message": "MCP stdio message could not be parsed",
            "data": { "yanshuCode": diagnostic.code },
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{LATEST_LEGACY_PROTOCOL_VERSION, MODERN_PROTOCOL_VERSION, McpServer};

    const SOURCE: &str = "(program (name mcp) (version 4) (signature id (fn (integer) integer)) (def id (fn (value) value)) (export id))";

    fn response(server: &mut McpServer, message: Value) -> Value {
        server
            .handle_message(&message)
            .unwrap_or_else(|| panic!("request unexpectedly produced no response"))
    }

    fn initialize(server: &mut McpServer) -> Value {
        response(
            server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": LATEST_LEGACY_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1" }
                }
            }),
        )
    }

    fn modern_params() -> Value {
        json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "1" },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        })
    }

    #[test]
    fn supports_legacy_initialize_list_and_structured_tool_call() {
        let mut server = McpServer::new();
        let initialized = initialize(&mut server);
        assert_eq!(
            initialized["result"]["protocolVersion"],
            LATEST_LEGACY_PROTOCOL_VERSION
        );
        assert!(
            initialized["result"]["instructions"]
                .as_str()
                .is_some_and(|value| value.starts_with("Read-only Yanshu"))
        );

        let listed = response(
            &mut server,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        );
        assert_eq!(listed["result"]["tools"].as_array().map_or(0, Vec::len), 3);
        assert!(listed["result"].get("resultType").is_none());

        let called = response(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "yanshu.inspect_source",
                    "arguments": { "source": SOURCE }
                }
            }),
        );
        assert_eq!(called["result"]["isError"], false);
        assert_eq!(called["result"]["structuredContent"]["ok"], true);
        let compatibility_text = called["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("compatibility text missing"));
        let decoded: Value = serde_json::from_str(compatibility_text)
            .unwrap_or_else(|error| panic!("compatibility text is not JSON: {error}"));
        assert_eq!(decoded, called["result"]["structuredContent"]);
    }

    #[test]
    fn supports_modern_discovery_and_stateless_tools() {
        let mut server = McpServer::new();
        let discovered = response(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": "discover",
                "method": "server/discover",
                "params": modern_params()
            }),
        );
        assert_eq!(discovered["result"]["resultType"], "complete");
        assert_eq!(
            discovered["result"]["supportedVersions"],
            json!([MODERN_PROTOCOL_VERSION])
        );

        let listed = response(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": modern_params()
            }),
        );
        assert_eq!(listed["result"]["resultType"], "complete");
        assert_eq!(listed["result"]["cacheScope"], "public");
    }

    #[test]
    fn separates_protocol_errors_from_actionable_tool_diagnostics() {
        let mut server = McpServer::new();
        let before_initialize = response(
            &mut server,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
        );
        assert_eq!(
            before_initialize["error"]["data"]["yanshuCode"],
            "MCP_NOT_INITIALIZED"
        );

        let _initialized = initialize(&mut server);
        let unknown = response(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "yanshu.write_file", "arguments": {} }
            }),
        );
        assert_eq!(unknown["error"]["code"], -32602);

        let invalid_source = response(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {
                    "name": "yanshu.review_source",
                    "arguments": { "source": "not a program" }
                }
            }),
        );
        assert_eq!(invalid_source["result"]["isError"], true);
        assert_eq!(invalid_source["result"]["structuredContent"]["ok"], false);
    }
}
