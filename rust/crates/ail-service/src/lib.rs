#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_runtime::{
    CapabilityHost, ExecutionOptions, MapKey, Value, execute_export_with_host, json_to_value,
};
use ail_store::atomic_replace;
use ail_syntax::{Program, Route};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map, Value as JsonValue, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequest {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, Value>,
    pub headers: BTreeMap<String, Value>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: JsonValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchResult {
    pub response: ServiceResponse,
    pub diagnostic: Option<JsonValue>,
    pub handler: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryKvStore {
    data: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileKvStore {
    path: PathBuf,
    data: BTreeMap<String, Value>,
}

impl FileKvStore {
    pub fn open(path: impl AsRef<Path>) -> AilResult<Self> {
        let path = path.as_ref().to_path_buf();
        let data = if path.exists() {
            read_store_file(&path)?
        } else {
            BTreeMap::new()
        };
        Ok(Self { path, data })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn handle_service_request(
    program: &Program,
    request: &ServiceRequest,
    store: &mut MemoryKvStore,
    clock_milliseconds: &BigInt,
) -> DispatchResult {
    handle_service_request_with_id(
        program,
        request,
        store,
        clock_milliseconds,
        "req-rust-compat",
    )
}

pub fn handle_service_request_with_id(
    program: &Program,
    request: &ServiceRequest,
    store: &mut MemoryKvStore,
    clock_milliseconds: &BigInt,
    request_id: &str,
) -> DispatchResult {
    let mut host = ServiceHost {
        working: store.data.clone(),
        clock_milliseconds: clock_milliseconds.clone(),
        logs: Vec::new(),
    };
    let result = dispatch_request_with_id(program, request, &mut host, request_id);
    if result.diagnostic.is_none() {
        store.data = host.working;
    }
    result
}

pub fn handle_file_service_request(
    program: &Program,
    request: &ServiceRequest,
    store: &mut FileKvStore,
    clock_milliseconds: &BigInt,
) -> DispatchResult {
    handle_file_service_request_with_id(
        program,
        request,
        store,
        clock_milliseconds,
        "req-rust-compat",
    )
}

pub fn handle_file_service_request_with_id(
    program: &Program,
    request: &ServiceRequest,
    store: &mut FileKvStore,
    clock_milliseconds: &BigInt,
    request_id: &str,
) -> DispatchResult {
    let mut memory = MemoryKvStore {
        data: store.data.clone(),
    };
    let result = handle_service_request_with_id(
        program,
        request,
        &mut memory,
        clock_milliseconds,
        request_id,
    );
    if result.diagnostic.is_some() {
        return result;
    }
    if let Err(diagnostic) = write_store_file(&store.path, &memory.data) {
        return internal_dispatch_result(request, &diagnostic, None, request_id);
    }
    store.data = memory.data;
    result
}

pub fn dispatch_request(
    program: &Program,
    request: &ServiceRequest,
    host: &mut dyn CapabilityHost,
) -> DispatchResult {
    dispatch_request_with_id(program, request, host, "req-rust-compat")
}

pub fn dispatch_request_with_id(
    program: &Program,
    request: &ServiceRequest,
    host: &mut dyn CapabilityHost,
    request_id: &str,
) -> DispatchResult {
    let method = request.method.to_uppercase();
    let path_matches = program
        .routes
        .iter()
        .filter_map(|route| {
            match_route_path(&route.path, &request.path).map(|params| (route, params))
        })
        .collect::<Vec<_>>();
    let selected = path_matches
        .iter()
        .find(|(route, _)| route.method == method);
    let Some((route, parameters)) = selected else {
        if path_matches.is_empty() {
            return public_error_response(
                404,
                "ROUTE_NOT_FOUND",
                "no route matches the request",
                None,
            );
        }
        let allow = path_matches
            .iter()
            .map(|(route, _)| route.method.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return public_error_response(
            405,
            "METHOD_NOT_ALLOWED",
            "method is not allowed for this path",
            Some(("allow", allow)),
        );
    };
    let guest_request = string_map([
        ("method", Value::String(method)),
        ("path", Value::String(request.path.clone())),
        (
            "params",
            Value::Map(
                parameters
                    .iter()
                    .map(|(key, value)| (MapKey::String(key.clone()), Value::String(value.clone())))
                    .collect(),
            ),
        ),
        ("query", string_value_map(&request.query)),
        ("headers", string_value_map(&request.headers)),
        ("body", request.body.clone()),
    ]);
    match execute_export_with_host(
        program,
        &route.handler,
        vec![guest_request],
        ExecutionOptions {
            fuel: 25_000,
            ..ExecutionOptions::default()
        },
        host,
    )
    .and_then(validate_guest_response)
    {
        Ok(response) => DispatchResult {
            response,
            diagnostic: None,
            handler: Some(route.handler.clone()),
        },
        Err(diagnostic) => internal_dispatch_result(request, &diagnostic, Some(route), request_id),
    }
}

pub fn run_service_suite(program: &Program, path: impl AsRef<Path>) -> AilResult<JsonValue> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "SERVICE_TEST_INVALID_DOCUMENT",
            "host could not read the service test suite",
            json!({ "path": path.display().to_string(), "kind": error.kind().to_string() }),
        )
    })?;
    let document: JsonValue = serde_json::from_str(&source).map_err(|_| {
        Diagnostic::simple(
            "SERVICE_TEST_INVALID_DOCUMENT",
            "service test suite must be a JSON object",
        )
    })?;
    let document = document.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "SERVICE_TEST_INVALID_DOCUMENT",
            "service test suite must be a JSON object",
        )
    })?;
    let clock = document
        .get("clockMs")
        .map_or_else(|| Ok(BigInt::from(1_700_000_000_000_i64)), json_integer)?;
    let cases = document
        .get("cases")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            Diagnostic::simple(
                "SERVICE_TEST_INVALID_CASES",
                "service test cases must be a JSON array",
            )
        })?;
    let mut store = MemoryKvStore::default();
    let mut failures = Vec::new();
    let mut passed_count = 0_usize;
    for (index, case) in cases.iter().enumerate() {
        match run_service_case(program, &mut store, &clock, case, index)? {
            Some(failure) => failures.push(failure),
            None => passed_count += 1,
        }
    }
    Ok(json!({
        "passed": failures.is_empty(),
        "total": cases.len(),
        "passedCount": passed_count,
        "failedCount": failures.len(),
        "failures": failures,
    }))
}

fn run_service_case(
    program: &Program,
    store: &mut MemoryKvStore,
    clock: &BigInt,
    value: &JsonValue,
    index: usize,
) -> AilResult<Option<JsonValue>> {
    let document = value.as_object().ok_or_else(|| {
        Diagnostic::new(
            "SERVICE_TEST_INVALID_CASE",
            "service test case must be a JSON object",
            json!({ "index": index }),
        )
    })?;
    let name = document
        .get("name")
        .and_then(JsonValue::as_str)
        .map_or_else(|| format!("case-{index}"), str::to_owned);
    let method = required_string(document, "method")?;
    let path = required_string(document, "path")?;
    let expected_status = document
        .get("expectStatus")
        .and_then(JsonValue::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (100..=599).contains(value))
        .ok_or_else(|| {
            Diagnostic::new(
                "SERVICE_TEST_INVALID_CASE",
                "service test case has invalid name, request, or status",
                json!({ "index": index }),
            )
        })?;
    let query = json_object_to_guest_map(document.get("query").unwrap_or(&json!({})))?;
    let headers = json_object_to_guest_map(document.get("headers").unwrap_or(&json!({})))?;
    let body = json_to_value(document.get("body").unwrap_or(&JsonValue::Null))?;
    let response = handle_service_request(
        program,
        &ServiceRequest {
            method: method.to_owned(),
            path: path.to_owned(),
            query,
            headers,
            body,
        },
        store,
        clock,
    )
    .response;
    if response.status != expected_status {
        return Ok(Some(json!({
            "name": name,
            "reason": "status-mismatch",
            "expectedStatus": expected_status,
            "actualStatus": response.status,
            "actualBody": response.body,
        })));
    }
    if let Some(expected) = document.get("expectBody")
        && response.body != *expected
    {
        return Ok(Some(json!({
            "name": name,
            "reason": "body-mismatch",
            "expected": expected,
            "actual": response.body,
        })));
    }
    if let Some(expected) = document.get("expectBodyContains")
        && !json_contains(&response.body, expected)
    {
        return Ok(Some(json!({
            "name": name,
            "reason": "body-does-not-contain",
            "expectedContains": expected,
            "actual": response.body,
        })));
    }
    Ok(None)
}

struct ServiceHost {
    working: BTreeMap<String, Value>,
    clock_milliseconds: BigInt,
    logs: Vec<Value>,
}

impl CapabilityHost for ServiceHost {
    fn supports(&self, capability: &str) -> bool {
        matches!(capability, "kv" | "clock" | "log")
    }

    fn invoke(&mut self, operation: &str, arguments: &[Value]) -> AilResult<Value> {
        match operation {
            "log" => {
                self.logs.push(arguments[0].clone());
                Ok(Value::Nil)
            }
            "now-ms" => Ok(Value::Int(self.clock_milliseconds.clone())),
            "kv-get" => {
                let key = expect_kv_key(operation, &arguments[0])?;
                Ok(self
                    .working
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| arguments[1].clone()))
            }
            "kv-put" => {
                let key = expect_kv_key(operation, &arguments[0])?.to_owned();
                let value = arguments[1].clone();
                self.working.insert(key, value.clone());
                Ok(value)
            }
            "kv-delete" => {
                let key = expect_kv_key(operation, &arguments[0])?;
                Ok(Value::Bool(self.working.remove(key).is_some()))
            }
            "kv-list" => {
                let prefix = expect_kv_key(operation, &arguments[0])?;
                let values = self
                    .working
                    .iter()
                    .filter(|(key, _)| key.starts_with(prefix))
                    .map(|(_, value)| value.clone())
                    .collect::<Vec<_>>();
                Ok(if values.is_empty() {
                    Value::Nil
                } else {
                    Value::List(values)
                })
            }
            _ => Err(Diagnostic::new(
                "RUNTIME_INVALID_CAPABILITY_BINDING",
                "host capability primitive is malformed",
                json!({ "primitive": operation }),
            )),
        }
    }
}

fn validate_guest_response(value: Value) -> AilResult<ServiceResponse> {
    let Value::Map(mapping) = value else {
        return Err(Diagnostic::simple(
            "SERVICE_INVALID_RESPONSE",
            "handler response must contain status, headers, and body",
        ));
    };
    if mapping.len() != 3 {
        return Err(Diagnostic::simple(
            "SERVICE_INVALID_RESPONSE",
            "handler response must contain status, headers, and body",
        ));
    }
    let status = compatible_get(&mapping, "status")?;
    let Value::Int(status) = status else {
        return Err(invalid_response_status());
    };
    let status = status
        .to_u16()
        .filter(|value| (100..=599).contains(value))
        .ok_or_else(invalid_response_status)?;
    let headers = compatible_get(&mapping, "headers")?;
    let Value::Map(headers) = headers else {
        return Err(Diagnostic::simple(
            "SERVICE_INVALID_RESPONSE_HEADERS",
            "handler response headers must be a map",
        ));
    };
    let mut normalized_headers = BTreeMap::new();
    for (key, value) in headers {
        let name = key.json_name();
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(Diagnostic::simple(
                "SERVICE_INVALID_RESPONSE_HEADERS",
                "response header name contains invalid characters",
            ));
        }
        let Value::String(value) = value else {
            return Err(Diagnostic::new(
                "SERVICE_INVALID_RESPONSE_HEADERS",
                "response header values must be strings",
                json!({ "header": name }),
            ));
        };
        if value.contains(['\r', '\n']) {
            return Err(Diagnostic::new(
                "SERVICE_INVALID_RESPONSE_HEADERS",
                "response header value contains a line break",
                json!({ "header": name }),
            ));
        }
        normalized_headers.insert(name.to_lowercase(), value.clone());
    }
    normalized_headers
        .entry("content-type".to_owned())
        .or_insert_with(|| "application/json; charset=utf-8".to_owned());
    Ok(ServiceResponse {
        status,
        headers: normalized_headers,
        body: compatible_get(&mapping, "body")?.to_json()?,
    })
}

fn compatible_get<'mapping>(
    mapping: &'mapping BTreeMap<MapKey, Value>,
    key: &str,
) -> AilResult<&'mapping Value> {
    mapping
        .get(&MapKey::String(key.to_owned()))
        .or_else(|| mapping.get(&MapKey::Symbol(key.to_owned())))
        .ok_or_else(|| {
            Diagnostic::new(
                "SERVICE_INVALID_RESPONSE",
                "handler response is missing a required field",
                json!({ "field": key }),
            )
        })
}

fn invalid_response_status() -> Diagnostic {
    Diagnostic::simple(
        "SERVICE_INVALID_RESPONSE_STATUS",
        "handler response status must be an integer from 100 through 599",
    )
}

fn match_route_path(pattern: &str, path: &str) -> Option<BTreeMap<String, String>> {
    let patterns = path_segments(pattern);
    let actuals = path_segments(path);
    if patterns.len() != actuals.len() {
        return None;
    }
    let mut parameters = BTreeMap::new();
    for (pattern, actual) in patterns.into_iter().zip(actuals) {
        if let Some(name) = pattern.strip_prefix(':') {
            parameters.insert(name.to_owned(), actual.to_owned());
        } else if pattern != actual {
            return None;
        }
    }
    Some(parameters)
}

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        Vec::new()
    } else {
        path.get(1..)
            .map_or_else(Vec::new, |value| value.split('/').collect())
    }
}

fn public_error_response(
    status: u16,
    code: &str,
    message: &str,
    extra_header: Option<(&str, String)>,
) -> DispatchResult {
    let mut headers = BTreeMap::from([(
        "content-type".to_owned(),
        "application/json; charset=utf-8".to_owned(),
    )]);
    if let Some((name, value)) = extra_header {
        headers.insert(name.to_owned(), value);
    }
    DispatchResult {
        response: ServiceResponse {
            status,
            headers,
            body: json!({
                "error": { "code": code, "message": message, "details": {} }
            }),
        },
        diagnostic: None,
        handler: None,
    }
}

fn internal_dispatch_result(
    request: &ServiceRequest,
    diagnostic: &Diagnostic,
    route: Option<&Route>,
    request_id: &str,
) -> DispatchResult {
    DispatchResult {
        response: ServiceResponse {
            status: 500,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "application/json; charset=utf-8".to_owned(),
            )]),
            body: json!({
                "error": {
                    "code": "INTERNAL_ERROR",
                    "message": "request could not be completed",
                    "details": { "requestId": request_id }
                }
            }),
        },
        diagnostic: Some(json!({
            "requestId": request_id,
            "method": request.method,
            "path": request.path,
            "handler": route.map(|value| value.handler.as_str()),
            "error": {
                "code": diagnostic.code,
                "message": diagnostic.message.as_ref(),
                "details": diagnostic.details.as_ref(),
            }
        })),
        handler: route.map(|value| value.handler.clone()),
    }
}

fn expect_kv_key<'value>(operation: &str, value: &'value Value) -> AilResult<&'value str> {
    let Value::String(key) = value else {
        return Err(invalid_kv_key(operation));
    };
    if key.is_empty() || key.chars().count() > 512 || key.contains('\0') {
        return Err(invalid_kv_key(operation));
    }
    Ok(key)
}

fn invalid_kv_key(operation: &str) -> Diagnostic {
    Diagnostic::new(
        "KV_INVALID_KEY",
        "KV key must be a non-empty bounded string",
        json!({ "operation": operation }),
    )
}

fn string_map<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (MapKey::String(key.to_owned()), value))
            .collect(),
    )
}

fn string_value_map(values: &BTreeMap<String, Value>) -> Value {
    Value::Map(
        values
            .iter()
            .map(|(key, value)| (MapKey::String(key.clone()), value.clone()))
            .collect(),
    )
}

fn json_object_to_guest_map(value: &JsonValue) -> AilResult<BTreeMap<String, Value>> {
    let object = value.as_object().ok_or_else(|| {
        Diagnostic::simple(
            "SERVICE_TEST_INVALID_CASE",
            "service test query and headers must be JSON objects",
        )
    })?;
    object
        .iter()
        .map(|(key, value)| Ok((key.clone(), json_to_value(value)?)))
        .collect()
}

fn json_integer(value: &JsonValue) -> AilResult<BigInt> {
    match value {
        JsonValue::Number(number) => BigInt::from_str(&number.to_string()).map_err(|_| {
            Diagnostic::simple(
                "SERVICE_TEST_INVALID_CLOCK",
                "service test clockMs must be an integer",
            )
        }),
        _ => Err(Diagnostic::simple(
            "SERVICE_TEST_INVALID_CLOCK",
            "service test clockMs must be an integer",
        )),
    }
}

fn required_string<'document>(
    document: &'document Map<String, JsonValue>,
    key: &str,
) -> AilResult<&'document str> {
    document
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                "SERVICE_TEST_MISSING_FIELD",
                "service test document is missing a required field",
                json!({ "field": key }),
            )
        })
}

fn json_contains(actual: &JsonValue, expected: &JsonValue) -> bool {
    match expected {
        JsonValue::Object(expected) => actual.as_object().is_some_and(|actual| {
            expected.iter().all(|(key, value)| {
                actual
                    .get(key)
                    .is_some_and(|actual_value| json_contains(actual_value, value))
            })
        }),
        JsonValue::Array(expected) => actual.as_array().is_some_and(|actual| {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| json_contains(actual, expected))
        }),
        _ => actual == expected,
    }
}

fn read_store_file(path: &Path) -> AilResult<BTreeMap<String, Value>> {
    let source = fs::read_to_string(path).map_err(|_| invalid_store_file(path))?;
    let document: JsonValue =
        serde_json::from_str(&source).map_err(|_| invalid_store_file(path))?;
    let document = document
        .as_object()
        .ok_or_else(|| invalid_store_shape(path))?;
    if document.get("version").and_then(JsonValue::as_u64) != Some(1) {
        return Err(invalid_store_shape(path));
    }
    let entries = document
        .get("entries")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| invalid_store_shape(path))?;
    let mut data = BTreeMap::new();
    for entry in entries {
        let entry = entry.as_object().ok_or_else(|| invalid_store_entry(path))?;
        let key = entry
            .get("key")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid_store_entry(path))?;
        let value = entry
            .get("value")
            .ok_or_else(|| invalid_store_entry(path))?;
        let key_value = Value::String(key.to_owned());
        let guest_key = expect_kv_key("load", &key_value)?.to_owned();
        if data.contains_key(&guest_key) {
            return Err(Diagnostic::new(
                "KV_INVALID_FILE",
                "KV persistence file contains a duplicate key",
                json!({ "path": path.display().to_string(), "key": guest_key }),
            ));
        }
        data.insert(guest_key, json_to_value(value)?);
    }
    Ok(data)
}

fn write_store_file(path: &Path, data: &BTreeMap<String, Value>) -> AilResult<()> {
    let entries = data
        .iter()
        .map(|(key, value)| Ok(json!({ "key": key, "value": value.to_json()? })))
        .collect::<AilResult<Vec<_>>>()?;
    let bytes = serde_json::to_vec(&json!({ "version": 1, "entries": entries }))
        .map_err(|_| store_write_failure(path))?;
    atomic_replace(path, &[bytes.as_slice(), b"\n"].concat()).map_err(|_| store_write_failure(path))
}

fn invalid_store_file(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "KV_INVALID_FILE",
        "KV persistence file is not valid JSON",
        json!({ "path": path.display().to_string() }),
    )
}

fn invalid_store_shape(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "KV_INVALID_FILE",
        "KV persistence file has an invalid document shape",
        json!({ "path": path.display().to_string() }),
    )
}

fn invalid_store_entry(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "KV_INVALID_FILE",
        "KV persistence entry is malformed",
        json!({ "path": path.display().to_string() }),
    )
}

fn store_write_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "KV_WRITE_FAILURE",
        "KV persistence file could not be replaced atomically",
        json!({ "path": path.display().to_string() }),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use ail_diagnostic::AilResult;
    use ail_runtime::Value as GuestValue;
    use ail_syntax::load_program_source;
    use serde_json::Value as JsonValue;

    use num_bigint::BigInt;

    use super::{
        FileKvStore, ServiceRequest, handle_file_service_request, json_object_to_guest_map,
        run_service_suite,
    };

    const TASK_SERVICE: &str = include_str!("../../../../examples/tasks/service.ail");

    fn require<T>(result: AilResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    #[test]
    fn rust_service_passes_all_task_business_scenarios() {
        let program = require(load_program_source(TASK_SERVICE));
        let scenarios =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/tasks/scenarios.json");
        let report = require(run_service_suite(&program, scenarios));
        assert_eq!(
            report.get("passed").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(report.get("total").and_then(JsonValue::as_u64), Some(11));
    }

    #[test]
    fn file_kv_survives_reopen_with_racket_compatible_json() {
        let program = require(load_program_source(TASK_SERVICE));
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        let clock = BigInt::from(1_700_000_000_000_i64);
        let create_body = serde_json::json!({
            "id": "persisted",
            "title": "stored",
            "completed": false
        });
        let create = ServiceRequest {
            method: "POST".to_owned(),
            path: "/tasks".to_owned(),
            query: Default::default(),
            headers: Default::default(),
            body: require(ail_runtime::json_to_value(&create_body)),
        };
        let mut first = require(FileKvStore::open(&store_path));
        assert_eq!(
            handle_file_service_request(&program, &create, &mut first, &clock)
                .response
                .status,
            201
        );
        drop(first);

        let mut reopened = require(FileKvStore::open(&store_path));
        let read = ServiceRequest {
            method: "GET".to_owned(),
            path: "/tasks/persisted".to_owned(),
            query: require(json_object_to_guest_map(&serde_json::json!({}))),
            headers: Default::default(),
            body: GuestValue::Nil,
        };
        let response = handle_file_service_request(&program, &read, &mut reopened, &clock).response;
        assert_eq!(response.status, 200);
        assert_eq!(
            response.body.get("title").and_then(JsonValue::as_str),
            Some("stored")
        );

        let persisted: JsonValue = require(
            fs::read_to_string(&store_path)
                .map_err(|_| ail_diagnostic::Diagnostic::simple("TEST_READ", "read failed"))
                .and_then(|source| {
                    serde_json::from_str(&source)
                        .map_err(|_| ail_diagnostic::Diagnostic::simple("TEST_JSON", "json failed"))
                }),
        );
        assert_eq!(
            persisted.get("version").and_then(JsonValue::as_u64),
            Some(1)
        );
    }

    #[test]
    fn file_kv_rejects_invalid_persistence_documents() {
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        fs::write(&store_path, b"{\"version\":2,\"entries\":[]}")
            .unwrap_or_else(|error| panic!("fixture write failed: {error}"));

        let diagnostic = match FileKvStore::open(&store_path) {
            Ok(_) => panic!("unsupported persistence version must fail closed"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(diagnostic.code, "KV_INVALID_FILE");
    }

    #[test]
    fn failed_file_replacement_does_not_commit_memory_or_leave_temporary_files() {
        let program = require(load_program_source(TASK_SERVICE));
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        let mut store = require(FileKvStore::open(&store_path));
        fs::create_dir(&store_path)
            .unwrap_or_else(|error| panic!("failure fixture directory failed: {error}"));
        let request = ServiceRequest {
            method: "POST".to_owned(),
            path: "/tasks".to_owned(),
            query: Default::default(),
            headers: Default::default(),
            body: require(ail_runtime::json_to_value(&serde_json::json!({
                "id": "must-not-commit",
                "title": "write failure",
                "completed": false
            }))),
        };

        let result = handle_file_service_request(
            &program,
            &request,
            &mut store,
            &BigInt::from(1_700_000_000_000_i64),
        );
        assert_eq!(result.response.status, 500);
        assert!(store.data.is_empty());
        let remaining = fs::read_dir(&temporary.path)
            .unwrap_or_else(|error| panic!("fixture listing failed: {error}"))
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec!["store.json"]);
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir()
                .join(format!("ai-lang-rust-kv-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}
