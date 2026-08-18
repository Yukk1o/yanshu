#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_runtime::{ExecutionOptions, MapKey, Value as GuestValue, execute_export};
use ail_syntax::{Program, load_program_source};
use num_bigint::BigInt;
use serde_json::{Map, Value, json};

pub fn run_manifest(path: impl AsRef<Path>) -> AilResult<Value> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "CONFORMANCE_MANIFEST_READ",
            "host could not read the conformance manifest",
            json!({ "path": path.display().to_string(), "kind": error.kind().to_string() }),
        )
    })?;
    let manifest: Value = serde_json::from_str(&source).map_err(|error| {
        Diagnostic::new(
            "CONFORMANCE_INVALID_MANIFEST",
            "conformance manifest is not valid JSON",
            json!({ "line": error.line(), "column": error.column() }),
        )
    })?;
    let document = manifest.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "CONFORMANCE_INVALID_MANIFEST",
            "conformance manifest has an invalid shape",
        )
    })?;
    if document.get("formatVersion").and_then(Value::as_u64) != Some(1) {
        return Err(Diagnostic::simple(
            "CONFORMANCE_INVALID_MANIFEST",
            "conformance manifest has an invalid shape",
        ));
    }
    let cases = document
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Diagnostic::simple(
                "CONFORMANCE_INVALID_MANIFEST",
                "conformance manifest has an invalid shape",
            )
        })?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let mut results = Vec::with_capacity(cases.len());
    let mut passed_count = 0_usize;
    for (index, case) in cases.iter().enumerate() {
        let result = run_case(root, case, index + 1)?;
        if result.get("passed").and_then(Value::as_bool) == Some(true) {
            passed_count += 1;
        }
        results.push(result);
    }
    Ok(json!({
        "formatVersion": 1,
        "passed": passed_count == results.len(),
        "total": results.len(),
        "passedCount": passed_count,
        "failedCount": results.len() - passed_count,
        "cases": results,
    }))
}

fn run_case(root: &Path, case: &Value, index: usize) -> AilResult<Value> {
    let document = case.as_object().ok_or_else(|| {
        Diagnostic::new(
            "CONFORMANCE_INVALID_CASE",
            "conformance case must be an object",
            json!({ "index": index }),
        )
    })?;
    let name = required_string(document, "name", &index.to_string())?;
    let phase = required_string(document, "phase", name)?;
    let source_relative = required_string(document, "source", name)?;
    let expected = document
        .get("expect")
        .filter(|value| value.is_object())
        .ok_or_else(|| {
            Diagnostic::new(
                "CONFORMANCE_INVALID_CASE",
                "conformance case requires an expected outcome",
                json!({ "name": name }),
            )
        })?;
    let source_path = root.join(PathBuf::from(source_relative));
    let source = fs::read_to_string(&source_path).map_err(|_| {
        Diagnostic::new(
            "CONFORMANCE_SOURCE_MISSING",
            "conformance source file does not exist",
            json!({ "name": name, "source": source_relative }),
        )
    })?;

    let actual = match load_program_source(&source) {
        Ok(program) => match phase {
            "load" | "inspect" => json!({
                "kind": "program",
                "program": program.summary_json(),
            }),
            "run" => match run_program_case(&program, document, name) {
                Ok(value) => value,
                Err(diagnostic) => diagnostic_outcome(&diagnostic),
            },
            _ => {
                return Err(Diagnostic::new(
                    "CONFORMANCE_INVALID_PHASE",
                    "conformance case has an unknown phase",
                    json!({ "name": name, "phase": phase }),
                ));
            }
        },
        Err(diagnostic) => diagnostic_outcome(&diagnostic),
    };
    Ok(json!({
        "name": name,
        "passed": actual == *expected,
        "expected": expected,
        "actual": actual,
    }))
}

fn run_program_case(
    program: &Program,
    document: &Map<String, Value>,
    name: &str,
) -> AilResult<Value> {
    let entry = required_string(document, "entry", name)?;
    let raw_arguments = document
        .get("args")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Diagnostic::new(
                "CONFORMANCE_INVALID_CASE",
                "run case args must be an array",
                json!({ "name": name }),
            )
        })?;
    let fuel = document.get("fuel").map_or(Some(10_000), Value::as_u64);
    let maximum_depth = document.get("maxDepth").map_or(Some(256), Value::as_u64);
    let (Some(fuel), Some(maximum_depth)) = (fuel, maximum_depth) else {
        return Err(Diagnostic::new(
            "CONFORMANCE_INVALID_CASE",
            "run case limits must be positive integers",
            json!({ "name": name }),
        ));
    };
    if fuel == 0 || maximum_depth == 0 {
        return Err(Diagnostic::new(
            "CONFORMANCE_INVALID_CASE",
            "run case limits must be positive integers",
            json!({ "name": name }),
        ));
    }
    let arguments = raw_arguments
        .iter()
        .map(fixture_to_value)
        .collect::<AilResult<Vec<_>>>()?;
    let result = execute_export(
        program,
        entry,
        arguments,
        ExecutionOptions {
            fuel,
            maximum_depth: usize::try_from(maximum_depth).unwrap_or(usize::MAX),
            reference_libraries: document.get("libraryBackends").and_then(Value::as_str)
                != Some("none"),
        },
    )?;
    Ok(json!({ "kind": "value", "value": value_to_fixture(&result)? }))
}

pub fn fixture_to_value(value: &Value) -> AilResult<GuestValue> {
    match value {
        Value::Null => Ok(GuestValue::Nil),
        Value::Bool(value) => Ok(GuestValue::Bool(*value)),
        Value::String(value) => Ok(GuestValue::String(value.clone())),
        Value::Number(value) => BigInt::from_str(&value.to_string())
            .map(GuestValue::Int)
            .map_err(|_| invalid_fixture("fixture number must be an exact integer")),
        Value::Array(values) if values.is_empty() => Ok(GuestValue::Nil),
        Value::Array(values) => values
            .iter()
            .map(fixture_to_value)
            .collect::<AilResult<Vec<_>>>()
            .map(GuestValue::List),
        Value::Object(document) => fixture_object_to_value(document),
    }
}

fn fixture_object_to_value(document: &Map<String, Value>) -> AilResult<GuestValue> {
    if document.len() == 1 {
        if let Some(raw) = document.get("$int").and_then(Value::as_str) {
            return BigInt::from_str(raw)
                .map(GuestValue::Int)
                .map_err(|_| invalid_fixture("$int fixture value must contain a decimal integer"));
        }
        if document.get("$nil").and_then(Value::as_bool) == Some(true) {
            return Ok(GuestValue::Nil);
        }
        if let Some(symbol) = document.get("$symbol").and_then(Value::as_str) {
            return Ok(GuestValue::Symbol(symbol.to_owned()));
        }
        if let Some(value) = document.get("$ok") {
            return Ok(GuestValue::Ok(Box::new(fixture_to_value(value)?)));
        }
        if let Some(value) = document.get("$err") {
            return Ok(GuestValue::Err(Box::new(fixture_to_value(value)?)));
        }
    }
    let mut mapping = BTreeMap::new();
    for (key, value) in document {
        mapping.insert(MapKey::String(key.clone()), fixture_to_value(value)?);
    }
    Ok(GuestValue::Map(mapping))
}

pub fn value_to_fixture(value: &GuestValue) -> AilResult<Value> {
    match value {
        GuestValue::Nil => Ok(json!({ "$nil": true })),
        GuestValue::Bool(value) => Ok(Value::Bool(*value)),
        GuestValue::Int(value) => Ok(json!({ "$int": value.to_string() })),
        GuestValue::String(value) => Ok(Value::String(value.clone())),
        GuestValue::Symbol(value) => Ok(json!({ "$symbol": value })),
        GuestValue::List(values) => values
            .iter()
            .map(value_to_fixture)
            .collect::<AilResult<Vec<_>>>()
            .map(Value::Array),
        GuestValue::Map(values) => {
            let mut document = Map::new();
            for (key, item) in values {
                document.insert(key.json_name().to_owned(), value_to_fixture(item)?);
            }
            Ok(Value::Object(document))
        }
        GuestValue::Ok(value) => Ok(json!({ "$ok": value_to_fixture(value)? })),
        GuestValue::Err(value) => Ok(json!({ "$err": value_to_fixture(value)? })),
        _ => Err(Diagnostic::new(
            "CONFORMANCE_UNSUPPORTED_VALUE",
            "guest value cannot be encoded as a conformance fixture",
            json!({ "kind": value.kind() }),
        )),
    }
}

fn diagnostic_outcome(diagnostic: &Diagnostic) -> Value {
    json!({
        "kind": "diagnostic",
        "code": diagnostic.code,
        "message": diagnostic.message.as_ref(),
        "details": diagnostic.details.as_ref(),
    })
}

fn required_string<'document>(
    document: &'document Map<String, Value>,
    key: &str,
    case_name: &str,
) -> AilResult<&'document str> {
    document
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Diagnostic::new(
                "CONFORMANCE_INVALID_CASE",
                "conformance case field must be a non-empty string",
                json!({ "case": case_name, "field": key }),
            )
        })
}

fn invalid_fixture(message: &'static str) -> Diagnostic {
    Diagnostic::simple("CONFORMANCE_INVALID_VALUE", message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ail_diagnostic::AilResult;
    use serde_json::Value;

    use super::run_manifest;

    fn require(result: AilResult<Value>) -> Value {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    #[test]
    fn rust_host_matches_every_v1_conformance_case() {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/v1/manifest.json");
        let report = require(run_manifest(manifest));
        let failed = report
            .get("cases")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|case| case.get("passed").and_then(Value::as_bool) != Some(true))
            .collect::<Vec<_>>();
        assert!(failed.is_empty(), "conformance failures: {failed:#?}");
        assert_eq!(report.get("total").and_then(Value::as_u64), Some(17));
    }
}
