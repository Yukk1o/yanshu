use serde_json::{Map, Value, json};
use yanshu_analysis::{analyze_program, render_rust_review};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_format::{FORMATTER_VERSION, FormatOptions, format_source};
use yanshu_syntax::{ReaderLimits, load_program_source};

pub(crate) const MAXIMUM_TOOL_SOURCE_BYTES: usize = 512 * 1024;
pub(crate) const MAXIMUM_REVIEW_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAXIMUM_TOOL_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const REVIEW_RENDERER: &str = "rust-readonly-v3";

pub(crate) fn definitions() -> Vec<Value> {
    vec![
        definition(
            "yanshu.inspect_source",
            "Inspect Yanshu source",
            "Parse one complete .yan program with the canonical Reader and Parser. Language v4 also returns type/effect and capability analysis. Read-only; does not execute code or access files.",
        ),
        definition(
            "yanshu.format_source",
            "Format Yanshu source",
            "Return a canonical formatter v1 candidate for one complete .yan program. Read-only; never writes a file, and fails closed if parsing, comments, meaning, or idempotence would change.",
        ),
        definition(
            "yanshu.review_source",
            "Review Yanshu source",
            "Return type/effect analysis and the generated Rust-style semantic review for one complete .yan program. The review is explicitly read-only, non-Rust, and never executable input.",
        ),
    ]
}

pub(crate) fn call(name: &str, arguments: &Value) -> YanshuResult<Value> {
    let source = source_argument(arguments)?;
    match name {
        "yanshu.inspect_source" => inspect(source),
        "yanshu.format_source" => format(source),
        "yanshu.review_source" => review(source),
        _ => Err(Diagnostic::simple(
            "MCP_TOOL_UNKNOWN",
            "MCP request named an unknown Yanshu tool",
        )),
    }
}

fn definition(name: &str, title: &str, description: &str) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "maxLength": MAXIMUM_TOOL_SOURCE_BYTES,
                    "description": "Current complete .yan source snapshot; maximum 512 KiB when UTF-8 encoded."
                }
            },
            "required": ["source"],
            "additionalProperties": false
        },
        "outputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"]
        },
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        }
    })
}

fn source_argument(arguments: &Value) -> YanshuResult<&str> {
    let object = arguments.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "MCP_TOOL_ARGUMENTS",
            "tool arguments must be one JSON object",
        )
    })?;
    reject_unknown_arguments(object)?;
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Diagnostic::simple(
                "MCP_TOOL_ARGUMENTS",
                "tool arguments must contain a string source field",
            )
        })?;
    if source.len() > MAXIMUM_TOOL_SOURCE_BYTES {
        return Err(Diagnostic::new(
            "MCP_SOURCE_LIMIT",
            "tool source exceeds the configured UTF-8 byte limit",
            json!({
                "actual": source.len(),
                "maximum": MAXIMUM_TOOL_SOURCE_BYTES,
            }),
        ));
    }
    Ok(source)
}

fn reject_unknown_arguments(arguments: &Map<String, Value>) -> YanshuResult<()> {
    if arguments.len() != 1 || !arguments.contains_key("source") {
        return Err(Diagnostic::simple(
            "MCP_TOOL_ARGUMENTS",
            "tool arguments may contain only the source field",
        ));
    }
    Ok(())
}

fn inspect(source: &str) -> YanshuResult<Value> {
    let program = load_program_source(source)?;
    let mut document = json!({ "ok": true, "program": program.inspect_json() });
    if program.version.to_string() == "4" {
        document["analysis"] = analyze_program(&program)?.to_json();
    }
    Ok(document)
}

fn format(source: &str) -> YanshuResult<Value> {
    let reader_limits = ReaderLimits {
        max_source_bytes: MAXIMUM_TOOL_SOURCE_BYTES,
        ..ReaderLimits::default()
    };
    let defaults = FormatOptions::default();
    let formatted = format_source(
        source,
        FormatOptions {
            max_output_bytes: MAXIMUM_TOOL_SOURCE_BYTES,
            reader_limits,
            ..defaults
        },
    )?;
    Ok(json!({
        "ok": true,
        "changed": formatted.changed,
        "formatterVersion": FORMATTER_VERSION,
        "formattedSource": formatted.source,
    }))
}

fn review(source: &str) -> YanshuResult<Value> {
    let program = load_program_source(source)?;
    let analysis = analyze_program(&program)?;
    let review = render_rust_review(&program, &analysis);
    if review.renderer != REVIEW_RENDERER || review.editable {
        return Err(Diagnostic::simple(
            "MCP_REVIEW_CONTRACT",
            "review renderer violated the read-only protocol contract",
        ));
    }
    if review.text.len() > MAXIMUM_REVIEW_TEXT_BYTES {
        return Err(Diagnostic::new(
            "MCP_REVIEW_TEXT_LIMIT",
            "review text exceeds the configured byte limit",
            json!({
                "actual": review.text.len(),
                "maximum": MAXIMUM_REVIEW_TEXT_BYTES,
            }),
        ));
    }
    Ok(json!({
        "ok": true,
        "analysis": analysis.to_json(),
        "review": review.to_json(),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MAXIMUM_TOOL_SOURCE_BYTES, call, definitions};

    const SOURCE: &str = "(program (name mcp) (version 4) (signature id (fn (integer) integer)) (def id (fn (value) value)) (export id))";

    #[test]
    fn exposes_only_read_only_source_tools() {
        let tools = definitions();
        assert_eq!(tools.len(), 3);
        for tool in tools {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["annotations"]["destructiveHint"], false);
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }

    #[test]
    fn inspect_format_and_review_use_canonical_apis() {
        let arguments = json!({ "source": SOURCE });
        let inspected = call("yanshu.inspect_source", &arguments)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(inspected["ok"], true);
        assert_eq!(inspected["program"]["version"], 4);

        let formatted = call("yanshu.format_source", &arguments)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(formatted["formatterVersion"], 1);
        assert!(formatted["formattedSource"].as_str().is_some());

        let reviewed = call("yanshu.review_source", &arguments)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(reviewed["review"]["renderer"], "rust-readonly-v3");
        assert_eq!(reviewed["review"]["editable"], false);
    }

    #[test]
    fn rejects_unknown_fields_and_oversized_utf8_before_parsing() {
        let unknown = call(
            "yanshu.inspect_source",
            &json!({ "source": SOURCE, "path": "policy.yan" }),
        )
        .err()
        .unwrap_or_else(|| panic!("unknown argument unexpectedly passed"));
        assert_eq!(unknown.code, "MCP_TOOL_ARGUMENTS");

        let source = "界".repeat(MAXIMUM_TOOL_SOURCE_BYTES / 3 + 1);
        let oversized = call("yanshu.inspect_source", &json!({ "source": source }))
            .err()
            .unwrap_or_else(|| panic!("oversized source unexpectedly passed"));
        assert_eq!(oversized.code, "MCP_SOURCE_LIMIT");
    }
}
