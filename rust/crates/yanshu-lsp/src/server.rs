use serde_json::{Map, Value, json};
use yanshu_diagnostic::Diagnostic;

use crate::document::DocumentStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Created,
    Running,
    Shutdown,
    Exited,
}

pub struct LanguageServer {
    lifecycle: Lifecycle,
    documents: DocumentStore,
}

impl Default for LanguageServer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageServer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            lifecycle: Lifecycle::Created,
            documents: DocumentStore::default(),
        }
    }

    #[must_use]
    pub fn should_exit(&self) -> bool {
        self.lifecycle == Lifecycle::Exited
    }

    #[must_use]
    pub fn handle_message(&mut self, message: &Value) -> Vec<Value> {
        let Some(object) = message.as_object() else {
            return vec![error_response(
                Value::Null,
                -32600,
                "JSON-RPC message must be an object",
                None,
            )];
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return vec![error_response(
                object.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "JSON-RPC version must be 2.0",
                None,
            )];
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return vec![error_response(
                object.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "JSON-RPC request or notification must contain a method",
                None,
            )];
        };
        let request_id = object.get("id").cloned();
        let params = object.get("params").unwrap_or(&Value::Null);

        if method == "exit" && request_id.is_none() {
            self.lifecycle = Lifecycle::Exited;
            return Vec::new();
        }
        if method == "initialize" {
            return self.initialize(request_id);
        }
        if self.lifecycle == Lifecycle::Created {
            return request_id.map_or_else(Vec::new, |id| {
                vec![error_response(
                    id,
                    -32002,
                    "server has not completed initialize",
                    None,
                )]
            });
        }
        if method == "shutdown" {
            return self.shutdown(request_id);
        }
        if self.lifecycle == Lifecycle::Shutdown {
            return request_id.map_or_else(Vec::new, |id| {
                vec![error_response(
                    id,
                    -32600,
                    "server is shut down and only accepts exit",
                    None,
                )]
            });
        }

        match (method, request_id) {
            ("initialized" | "$/cancelRequest", None) => Vec::new(),
            ("textDocument/didOpen", None) => self.did_open(params),
            ("textDocument/didChange", None) => self.did_change(params),
            ("textDocument/didClose", None) => self.did_close(params),
            ("textDocument/hover", Some(id)) => self.hover(id, params),
            ("textDocument/definition", Some(id)) => self.definition(id, params),
            ("textDocument/references", Some(id)) => self.references(id, params),
            ("textDocument/formatting", Some(id)) => self.formatting(id, params),
            (_, Some(id)) => vec![error_response(
                id,
                -32601,
                "method is not supported by yanshu-lsp",
                None,
            )],
            (_, None) => Vec::new(),
        }
    }

    fn initialize(&mut self, request_id: Option<Value>) -> Vec<Value> {
        let Some(id) = request_id else {
            return Vec::new();
        };
        if self.lifecycle != Lifecycle::Created {
            return vec![error_response(
                id,
                -32600,
                "initialize may only be requested once",
                None,
            )];
        }
        self.lifecycle = Lifecycle::Running;
        vec![success_response(
            id,
            json!({
                "capabilities": {
                    "positionEncoding": "utf-16",
                    "textDocumentSync": { "openClose": true, "change": 1 },
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "documentFormattingProvider": true,
                },
                "serverInfo": {
                    "name": "yanshu-lsp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )]
    }

    fn shutdown(&mut self, request_id: Option<Value>) -> Vec<Value> {
        let Some(id) = request_id else {
            return Vec::new();
        };
        if self.lifecycle != Lifecycle::Running {
            return vec![error_response(
                id,
                -32600,
                "shutdown requires a running server",
                None,
            )];
        }
        self.lifecycle = Lifecycle::Shutdown;
        vec![success_response(id, Value::Null)]
    }

    fn did_open(&mut self, params: &Value) -> Vec<Value> {
        let parsed = (|| {
            let document = object_field(params, "textDocument")?;
            let uri = string_field(document, "uri")?;
            let version = integer_field(document, "version")?;
            let text = string_field(document, "text")?;
            self.documents.open(uri, version, text)?;
            Ok::<_, Diagnostic>((uri.to_owned(), version))
        })();
        self.document_notification_result(parsed)
    }

    fn did_change(&mut self, params: &Value) -> Vec<Value> {
        let parsed = (|| {
            let document = object_field(params, "textDocument")?;
            let uri = string_field(document, "uri")?;
            let version = integer_field(document, "version")?;
            let changes = params
                .get("contentChanges")
                .and_then(Value::as_array)
                .ok_or_else(invalid_params)?;
            let [change] = changes.as_slice() else {
                return Err(invalid_params());
            };
            if change.get("range").is_some() {
                return Err(Diagnostic::simple(
                    "LSP_INCREMENTAL_CHANGE",
                    "yanshu-lsp only accepts full document synchronization",
                ));
            }
            let text = string_field(change, "text")?;
            self.documents.change(uri, version, text)?;
            Ok::<_, Diagnostic>((uri.to_owned(), version))
        })();
        self.document_notification_result(parsed)
    }

    fn did_close(&mut self, params: &Value) -> Vec<Value> {
        let parsed = (|| {
            let document = object_field(params, "textDocument")?;
            let uri = string_field(document, "uri")?;
            self.documents.close(uri);
            Ok::<_, Diagnostic>(uri.to_owned())
        })();
        match parsed {
            Ok(uri) => vec![publish_diagnostics(&uri, None, Vec::new())],
            Err(diagnostic) => vec![log_diagnostic(&diagnostic)],
        }
    }

    fn document_notification_result(
        &self,
        result: Result<(String, i64), Diagnostic>,
    ) -> Vec<Value> {
        match result {
            Ok((uri, version)) => {
                let diagnostics = self
                    .documents
                    .get(&uri)
                    .map_or_else(Vec::new, |document| document.diagnostics());
                vec![publish_diagnostics(&uri, Some(version), diagnostics)]
            }
            Err(diagnostic) => vec![log_diagnostic(&diagnostic)],
        }
    }

    fn hover(&self, id: Value, params: &Value) -> Vec<Value> {
        match text_position(params) {
            Ok((uri, line, character)) => vec![success_response(
                id,
                self.documents
                    .get(uri)
                    .and_then(|document| document.hover(line, character))
                    .unwrap_or(Value::Null),
            )],
            Err(diagnostic) => vec![invalid_params_response(id, &diagnostic)],
        }
    }

    fn definition(&self, id: Value, params: &Value) -> Vec<Value> {
        match text_position(params) {
            Ok((uri, line, character)) => vec![success_response(
                id,
                self.documents
                    .get(uri)
                    .and_then(|document| document.definition(line, character))
                    .unwrap_or(Value::Null),
            )],
            Err(diagnostic) => vec![invalid_params_response(id, &diagnostic)],
        }
    }

    fn references(&self, id: Value, params: &Value) -> Vec<Value> {
        let result = (|| {
            let (uri, line, character) = text_position(params)?;
            let context = object_field(params, "context")?;
            let include_declaration = boolean_field(context, "includeDeclaration")?;
            Ok::<_, Diagnostic>(
                self.documents
                    .get(uri)
                    .map_or(Ok(None), |document| {
                        document.references(line, character, include_declaration)
                    })?
                    .map_or(Value::Null, Value::Array),
            )
        })();
        match result {
            Ok(references) => vec![success_response(id, references)],
            Err(diagnostic) => vec![invalid_params_response(id, &diagnostic)],
        }
    }

    fn formatting(&self, id: Value, params: &Value) -> Vec<Value> {
        let result = (|| {
            let document = object_field(params, "textDocument")?;
            let uri = string_field(document, "uri")?;
            let open = self.documents.get(uri).ok_or_else(|| {
                Diagnostic::simple(
                    "LSP_DOCUMENT_UNKNOWN",
                    "formatting requires an open document",
                )
            })?;
            open.formatting_edits()
        })();
        match result {
            Ok(edits) => vec![success_response(id, Value::Array(edits))],
            Err(diagnostic) => vec![invalid_params_response(id, &diagnostic)],
        }
    }
}

fn object_field<'value>(value: &'value Value, name: &str) -> Result<&'value Value, Diagnostic> {
    value
        .as_object()
        .and_then(|object| object.get(name))
        .filter(|field| field.is_object())
        .ok_or_else(invalid_params)
}

fn string_field<'value>(value: &'value Value, name: &str) -> Result<&'value str, Diagnostic> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(invalid_params)
}

fn integer_field(value: &Value, name: &str) -> Result<i64, Diagnostic> {
    value
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(invalid_params)
}

fn boolean_field(value: &Value, name: &str) -> Result<bool, Diagnostic> {
    value
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(invalid_params)
}

fn text_position(params: &Value) -> Result<(&str, u64, u64), Diagnostic> {
    let document = object_field(params, "textDocument")?;
    let position = object_field(params, "position")?;
    let uri = string_field(document, "uri")?;
    let line = position
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(invalid_params)?;
    let character = position
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(invalid_params)?;
    Ok((uri, line, character))
}

fn invalid_params() -> Diagnostic {
    Diagnostic::simple(
        "LSP_INVALID_PARAMS",
        "LSP method parameters do not match the supported protocol shape",
    )
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = Map::new();
    error.insert("code".to_owned(), Value::from(code));
    error.insert("message".to_owned(), Value::String(message.to_owned()));
    if let Some(data) = data {
        error.insert("data".to_owned(), data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn invalid_params_response(id: Value, diagnostic: &Diagnostic) -> Value {
    error_response(
        id,
        -32602,
        "request parameters were rejected",
        Some(json!({ "yanshuCode": diagnostic.code })),
    )
}

fn publish_diagnostics(uri: &str, version: Option<i64>, diagnostics: Vec<Value>) -> Value {
    let mut params = json!({ "uri": uri, "diagnostics": diagnostics });
    if let Some(version) = version {
        params["version"] = Value::from(version);
    }
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": params,
    })
}

fn log_diagnostic(diagnostic: &Diagnostic) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "window/logMessage",
        "params": {
            "type": 1,
            "message": format!("{}: {}", diagnostic.code, diagnostic.message),
        },
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::LanguageServer;

    fn initialize(server: &mut LanguageServer) -> Value {
        server.handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {},
        }))[0]
            .clone()
    }

    #[test]
    fn lifecycle_advertises_only_implemented_capabilities() {
        let mut server = LanguageServer::new();
        let initialized = initialize(&mut server);
        assert_eq!(
            initialized["result"]["capabilities"]["positionEncoding"],
            "utf-16"
        );
        assert_eq!(
            initialized["result"]["capabilities"]["textDocumentSync"]["change"],
            1
        );
        assert_eq!(
            initialized["result"]["capabilities"]["referencesProvider"],
            true
        );
        let shutdown = server.handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
        }));
        assert_eq!(shutdown[0]["result"], Value::Null);
        let _outputs = server.handle_message(&json!({ "jsonrpc": "2.0", "method": "exit" }));
        assert!(server.should_exit());
    }

    #[test]
    fn open_change_and_close_publish_versioned_diagnostics() {
        let mut server = LanguageServer::new();
        initialize(&mut server);
        let opened = server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///broken.yan",
                    "languageId": "yanshu",
                    "version": 1,
                    "text": "(program",
                }
            }
        }));
        assert_eq!(opened[0]["params"]["version"], 1);
        assert_eq!(opened[0]["params"]["diagnostics"][0]["code"], "READ_SYNTAX");

        let changed = server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "file:///broken.yan", "version": 2 },
                "contentChanges": [{
                    "text": "(program (name fixed) (version 1) (def value (fn () 1)) (export value))"
                }]
            }
        }));
        assert_eq!(changed[0]["params"]["diagnostics"], json!([]));

        let closed = server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": "file:///broken.yan" } }
        }));
        assert_eq!(closed[0]["params"]["diagnostics"], json!([]));
    }

    #[test]
    fn hover_definition_references_and_formatting_use_the_open_snapshot() {
        let source = "(program (name tools) (version 4) (signature target (fn (integer) integer)) (def target (fn (x) x)) (signature use (fn (integer) integer)) (def use (fn (x) (target x))) (export target use))";
        let mut server = LanguageServer::new();
        initialize(&mut server);
        let _opened = server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tools.yan", "languageId": "yanshu", "version": 1, "text": source
            }}
        }));
        let call_offset = source
            .rfind("target x")
            .unwrap_or_else(|| panic!("target call fixture missing"));
        let character = source[..call_offset].encode_utf16().count();
        let position = json!({ "line": 0, "character": character });

        let definition = server.handle_message(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "file:///tools.yan" }, "position": position }
        }));
        assert_eq!(definition[0]["result"]["uri"], "file:///tools.yan");

        let references = server.handle_message(&json!({
            "jsonrpc": "2.0", "id": 5, "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": "file:///tools.yan" },
                "position": position,
                "context": { "includeDeclaration": true }
            }
        }));
        assert_eq!(references[0]["result"].as_array().map(Vec::len), Some(4));
        assert!(references[0]["result"].as_array().is_some_and(|locations| {
            locations
                .iter()
                .all(|location| location["uri"] == "file:///tools.yan")
        }));

        let hover = server.handle_message(&json!({
            "jsonrpc": "2.0", "id": 3, "method": "textDocument/hover",
            "params": { "textDocument": { "uri": "file:///tools.yan" }, "position": position }
        }));
        assert!(
            hover[0]["result"]["contents"]["value"]
                .as_str()
                .is_some_and(|value| value.contains("node: expression-v1"))
        );

        let formatting = server.handle_message(&json!({
            "jsonrpc": "2.0", "id": 4, "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": "file:///tools.yan" },
                "options": { "tabSize": 2, "insertSpaces": true }
            }
        }));
        assert_eq!(formatting[0]["result"].as_array().map(Vec::len), Some(1));
    }
}
