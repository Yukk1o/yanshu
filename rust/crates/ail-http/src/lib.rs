#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    future::Future,
    io,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_runtime::{Value as GuestValue, json_to_value};
use ail_service::{
    DispatchResult, FileKvStore, ServiceRequest, ServiceResponse,
    handle_file_service_request_with_id,
};
use ail_store::VersionStore;
use ail_syntax::{Program, load_program_source};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
    response::Response,
};
use num_bigint::BigInt;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::Semaphore, task, time};
use zeroize::Zeroizing;

pub trait ProgramLoader: Send + Sync {
    fn load(&self) -> AilResult<Program>;
}

#[derive(Debug, Clone)]
pub struct FixedProgramLoader {
    program: Program,
}

impl FixedProgramLoader {
    pub fn from_source(source: &str) -> AilResult<Self> {
        Ok(Self {
            program: load_program_source(source)?,
        })
    }
}

impl ProgramLoader for FixedProgramLoader {
    fn load(&self) -> AilResult<Program> {
        Ok(self.program.clone())
    }
}

#[derive(Debug)]
pub struct ActiveVersionLoader {
    store: VersionStore,
}

impl ActiveVersionLoader {
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            store: VersionStore::new(root),
        }
    }
}

impl ProgramLoader for ActiveVersionLoader {
    fn load(&self) -> AilResult<Program> {
        load_program_source(&self.store.active_source()?)
    }
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub maximum_target_bytes: usize,
    pub maximum_header_bytes: usize,
    pub maximum_headers: usize,
    pub maximum_body_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_concurrency: usize,
    pub body_read_timeout: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            maximum_target_bytes: 8 * 1024,
            maximum_header_bytes: 64 * 1024,
            maximum_headers: 100,
            maximum_body_bytes: 1024 * 1024,
            maximum_response_bytes: 1024 * 1024,
            maximum_concurrency: 32,
            body_read_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone)]
struct HttpState {
    loader: Arc<dyn ProgramLoader>,
    store: Arc<Mutex<FileKvStore>>,
    permits: Arc<Semaphore>,
    config: HttpConfig,
    authentication: Option<Arc<BearerAuth>>,
}

pub struct BearerAuth {
    token_digest: [u8; 32],
}

impl BearerAuth {
    pub fn new(token: String) -> AilResult<Self> {
        let token = Zeroizing::new(token);
        if token.is_empty() || token.trim() != token.as_str() {
            return Err(Diagnostic::simple(
                "HTTP_INVALID_AUTH_CONFIG",
                "HTTP Bearer token must be a non-empty value without surrounding whitespace",
            ));
        }
        Ok(Self {
            token_digest: Sha256::digest(token.as_bytes()).into(),
        })
    }

    fn authorizes(&self, headers: &HeaderMap) -> bool {
        let values = headers.get_all(AUTHORIZATION).iter().collect::<Vec<_>>();
        let [value] = values.as_slice() else {
            return false;
        };
        let Some(value) = value.to_str().ok() else {
            return false;
        };
        let Some((scheme, token)) = value.split_once(' ') else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
            return false;
        }
        let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        self.token_digest.ct_eq(&candidate).unwrap_u8() == 1
    }
}

pub fn build_router(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
) -> AilResult<Router> {
    build_router_with_auth(loader, store, config, None)
}

pub fn build_router_with_auth(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
) -> AilResult<Router> {
    validate_config(&config)?;
    let state = HttpState {
        loader,
        store: Arc::new(Mutex::new(store)),
        permits: Arc::new(Semaphore::new(config.maximum_concurrency)),
        config,
        authentication: authentication.map(Arc::new),
    };
    Ok(Router::new().fallback(dispatch).with_state(state))
}

pub fn build_active_router(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
) -> AilResult<Router> {
    build_active_router_with_auth(code_store, data_store, config, None)
}

pub fn build_active_router_with_auth(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
) -> AilResult<Router> {
    let loader: Arc<dyn ProgramLoader> = Arc::new(ActiveVersionLoader::new(code_store));
    let store = FileKvStore::open(data_store)?;
    build_router_with_auth(loader, store, config, authentication)
}

pub async fn serve_with_shutdown<Shutdown>(
    listener: TcpListener,
    router: Router,
    shutdown: Shutdown,
) -> io::Result<()>
where
    Shutdown: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn dispatch(State(state): State<HttpState>, request: Request) -> Response {
    let request_id = match generate_request_id() {
        Ok(request_id) => request_id,
        Err(diagnostic) => {
            return protocol_error_response(StatusCode::INTERNAL_SERVER_ERROR, &diagnostic, None);
        }
    };
    let mut response = dispatch_identified(state, request, &request_id).await;
    if let Ok(value) = HeaderValue::try_from(request_id.as_str()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), value);
    }
    response
}

async fn dispatch_identified(state: HttpState, request: Request, request_id: &str) -> Response {
    if let Some(authentication) = &state.authentication
        && !authentication.authorizes(request.headers())
    {
        let mut response = protocol_error_response(
            StatusCode::UNAUTHORIZED,
            &Diagnostic::simple(
                "HTTP_AUTH_REQUIRED",
                "valid Bearer authentication is required",
            ),
            Some(request_id),
        );
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }
    let Ok(_permit) = Arc::clone(&state.permits).try_acquire_owned() else {
        return protocol_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &Diagnostic::simple("HTTP_BUSY", "server concurrency limit is exhausted"),
            Some(request_id),
        );
    };
    let method_is_head = request.method() == axum::http::Method::HEAD;
    let service_request = match parse_request(request, &state.config).await {
        Ok(request) => request,
        Err(diagnostic) => {
            return protocol_error_response(
                diagnostic_status(&diagnostic),
                &diagnostic,
                Some(request_id),
            );
        }
    };
    let worker_state = state.clone();
    let worker_request_id = request_id.to_owned();
    let result = task::spawn_blocking(move || {
        execute_request(&worker_state, &service_request, &worker_request_id)
    })
    .await;
    match result {
        Ok(Ok(result)) => dispatch_response(
            result,
            method_is_head,
            state.config.maximum_response_bytes,
            request_id,
        ),
        Ok(Err(diagnostic)) => protocol_error_response(
            diagnostic_status(&diagnostic),
            &diagnostic,
            Some(request_id),
        ),
        Err(_) => protocol_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &Diagnostic::simple(
                "HTTP_WORKER_FAILURE",
                "request worker could not be completed",
            ),
            Some(request_id),
        ),
    }
}

fn execute_request(
    state: &HttpState,
    request: &ServiceRequest,
    request_id: &str,
) -> AilResult<DispatchResult> {
    let program = state.loader.load().map_err(|_| {
        Diagnostic::simple("HTTP_SERVICE_UNAVAILABLE", "service program is unavailable")
    })?;
    let mut store = state.store.lock().map_err(|_| {
        Diagnostic::simple(
            "HTTP_STORE_UNAVAILABLE",
            "service data store is unavailable",
        )
    })?;
    Ok(handle_file_service_request_with_id(
        &program,
        request,
        &mut store,
        &current_milliseconds(),
        request_id,
    ))
}

async fn parse_request(request: Request, config: &HttpConfig) -> AilResult<ServiceRequest> {
    let (parts, body) = request.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map_or(parts.uri.path(), |value| value.as_str());
    if target.len() > config.maximum_target_bytes {
        return Err(Diagnostic::new(
            "HTTP_REQUEST_LINE_TOO_LARGE",
            "request target exceeded the byte limit",
            json!({ "limitBytes": config.maximum_target_bytes }),
        ));
    }
    let headers = parse_headers(&parts.headers, config)?;
    if let Some(length) = parts.headers.get(CONTENT_LENGTH) {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                Diagnostic::new(
                    "HTTP_INVALID_CONTENT_LENGTH",
                    "Content-Length is invalid or exceeds the body limit",
                    json!({ "limitBytes": config.maximum_body_bytes }),
                )
            })?;
        if length > config.maximum_body_bytes {
            return Err(Diagnostic::new(
                "HTTP_INVALID_CONTENT_LENGTH",
                "Content-Length is invalid or exceeds the body limit",
                json!({ "limitBytes": config.maximum_body_bytes }),
            ));
        }
    }
    let bytes = match time::timeout(
        config.body_read_timeout,
        to_bytes(body, config.maximum_body_bytes),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => {
            return Err(Diagnostic::new(
                "HTTP_INVALID_CONTENT_LENGTH",
                "request body exceeds the body limit",
                json!({ "limitBytes": config.maximum_body_bytes }),
            ));
        }
        Err(_) => {
            return Err(Diagnostic::simple(
                "HTTP_REQUEST_TIMEOUT",
                "HTTP request exceeded its read deadline",
            ));
        }
    };
    if !bytes.is_empty() && !has_json_content_type(&parts.headers) {
        return Err(Diagnostic::simple(
            "HTTP_UNSUPPORTED_MEDIA_TYPE",
            "request body must use application/json",
        ));
    }
    let body = if bytes.is_empty() {
        GuestValue::Nil
    } else {
        let document: JsonValue = serde_json::from_slice(&bytes).map_err(|_| {
            Diagnostic::simple("HTTP_INVALID_JSON", "request body is not valid JSON")
        })?;
        json_to_value(&document)?
    };
    let path = decode_path(parts.uri.path())?;
    let query = decode_query(parts.uri.query().unwrap_or(""))?;
    Ok(ServiceRequest {
        method: parts.method.as_str().to_uppercase(),
        path,
        query,
        headers,
        body,
    })
}

fn parse_headers(
    headers: &HeaderMap,
    config: &HttpConfig,
) -> AilResult<BTreeMap<String, GuestValue>> {
    if headers.keys().count() > config.maximum_headers {
        return Err(Diagnostic::simple(
            "HTTP_TOO_MANY_HEADERS",
            "request contains too many headers",
        ));
    }
    let mut total_bytes = 0_usize;
    let mut result = BTreeMap::new();
    for name in headers.keys() {
        let values = headers.get_all(name).iter().collect::<Vec<_>>();
        if values.len() != 1 {
            return Err(Diagnostic::new(
                "HTTP_DUPLICATE_HEADER",
                "duplicate request headers are not supported",
                json!({ "header": name.as_str() }),
            ));
        }
        let value = values[0].to_str().map_err(|_| {
            Diagnostic::simple("HTTP_INVALID_TEXT", "HTTP header is not valid UTF-8")
        })?;
        total_bytes = total_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.len())
            .saturating_add(4);
        if total_bytes > config.maximum_header_bytes {
            return Err(Diagnostic::new(
                "HTTP_HEADERS_TOO_LARGE",
                "request headers exceeded the byte limit",
                json!({ "limitBytes": config.maximum_header_bytes }),
            ));
        }
        if is_sensitive_request_header(name.as_str()) {
            continue;
        }
        result.insert(
            name.as_str().to_owned(),
            GuestValue::String(value.trim().to_owned()),
        );
    }
    Ok(result)
}

fn is_sensitive_request_header(name: &str) -> bool {
    matches!(
        name,
        "authorization" | "cookie" | "proxy-authorization" | "x-api-key" | "x-request-id"
    )
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn decode_path(raw: &str) -> AilResult<String> {
    if raw == "/" {
        return Ok("/".to_owned());
    }
    if !raw.starts_with('/') {
        return Err(invalid_path_encoding());
    }
    let segments = raw[1..]
        .split('/')
        .map(|segment| {
            let decoded = decode_component(segment, false).map_err(|_| invalid_path_encoding())?;
            if decoded.contains('/') {
                return Err(invalid_path_encoding());
            }
            Ok(decoded)
        })
        .collect::<AilResult<Vec<_>>>()?;
    Ok(format!("/{}", segments.join("/")))
}

fn decode_query(raw: &str) -> AilResult<BTreeMap<String, GuestValue>> {
    let mut result = BTreeMap::new();
    if raw.is_empty() {
        return Ok(result);
    }
    for pair in raw.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_component(raw_key, true).map_err(|_| invalid_query_encoding())?;
        let value = decode_component(raw_value, true).map_err(|_| invalid_query_encoding())?;
        result.insert(key, GuestValue::String(value));
    }
    Ok(result)
}

fn decode_component(raw: &str, plus_is_space: bool) -> Result<String, ()> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_digit(bytes[index + 1]).ok_or(())?;
                let low = hex_digit(bytes[index + 2]).ok_or(())?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'%' => return Err(()),
            b'+' if plus_is_space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn dispatch_response(
    result: DispatchResult,
    head: bool,
    maximum_bytes: usize,
    request_id: &str,
) -> Response {
    service_response_to_http(result.response, head, maximum_bytes).unwrap_or_else(|diagnostic| {
        protocol_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &diagnostic,
            Some(request_id),
        )
    })
}

fn service_response_to_http(
    response: ServiceResponse,
    head: bool,
    maximum_bytes: usize,
) -> AilResult<Response> {
    let bytes = serde_json::to_vec(&response.body).map_err(|_| {
        Diagnostic::simple(
            "HTTP_RESPONSE_ENCODING",
            "response body could not be encoded",
        )
    })?;
    if bytes.len() > maximum_bytes {
        return Err(Diagnostic::new(
            "HTTP_RESPONSE_TOO_LARGE",
            "response body exceeded the byte limit",
            json!({ "limitBytes": maximum_bytes }),
        ));
    }
    let status = StatusCode::from_u16(response.status)
        .map_err(|_| Diagnostic::simple("HTTP_RESPONSE_STATUS", "response status is invalid"))?;
    let mut output = Response::new(if head {
        Body::empty()
    } else {
        Body::from(bytes)
    });
    *output.status_mut() = status;
    for (name, value) in response.headers {
        let name = HeaderName::try_from(name).map_err(|_| {
            Diagnostic::simple("HTTP_RESPONSE_HEADER", "response header name is invalid")
        })?;
        let value = HeaderValue::try_from(value).map_err(|_| {
            Diagnostic::simple("HTTP_RESPONSE_HEADER", "response header value is invalid")
        })?;
        output.headers_mut().insert(name, value);
    }
    Ok(output)
}

fn protocol_error_response(
    status: StatusCode,
    diagnostic: &Diagnostic,
    request_id: Option<&str>,
) -> Response {
    let details = request_id.map_or_else(|| json!({}), |value| json!({ "requestId": value }));
    let document = json!({
        "error": {
            "code": diagnostic.code,
            "message": diagnostic.message.as_ref(),
            "details": details,
        }
    });
    let body = serde_json::to_vec(&document).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn diagnostic_status(diagnostic: &Diagnostic) -> StatusCode {
    match diagnostic.code {
        "HTTP_AUTH_REQUIRED" => StatusCode::UNAUTHORIZED,
        "HTTP_REQUEST_TIMEOUT" => StatusCode::REQUEST_TIMEOUT,
        "HTTP_UNSUPPORTED_MEDIA_TYPE" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "HTTP_INVALID_CONTENT_LENGTH"
        | "HTTP_HEADERS_TOO_LARGE"
        | "HTTP_REQUEST_LINE_TOO_LARGE"
        | "HTTP_TOO_MANY_HEADERS" => StatusCode::PAYLOAD_TOO_LARGE,
        "HTTP_SERVICE_UNAVAILABLE" | "HTTP_STORE_UNAVAILABLE" | "HTTP_BUSY" => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::BAD_REQUEST,
    }
}

fn generate_request_id() -> AilResult<String> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| {
        Diagnostic::simple(
            "HTTP_RANDOM_FAILURE",
            "HTTP request identifier could not be generated",
        )
    })?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(36);
    output.push_str("req-");
    for byte in random {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

fn invalid_path_encoding() -> Diagnostic {
    Diagnostic::simple(
        "HTTP_INVALID_PATH_ENCODING",
        "request path contains invalid escaping",
    )
}

fn invalid_query_encoding() -> Diagnostic {
    Diagnostic::simple(
        "HTTP_INVALID_QUERY_ENCODING",
        "query string contains invalid escaping",
    )
}

fn current_milliseconds() -> BigInt {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    BigInt::from(milliseconds)
}

fn validate_config(config: &HttpConfig) -> AilResult<()> {
    if config.maximum_target_bytes == 0
        || config.maximum_header_bytes == 0
        || config.maximum_headers == 0
        || config.maximum_body_bytes == 0
        || config.maximum_response_bytes == 0
        || config.maximum_concurrency == 0
        || config.body_read_timeout.is_zero()
    {
        return Err(Diagnostic::simple(
            "HTTP_INVALID_CONFIG",
            "HTTP limits and timeouts must be positive",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use ail_diagnostic::AilResult;
    use ail_store::{CandidateRegistration, VersionStore};
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{
            HeaderMap, HeaderValue, Request, StatusCode,
            header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, WWW_AUTHENTICATE},
        },
        response::Response,
    };
    use serde_json::{Value as JsonValue, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::oneshot,
    };
    use tower::ServiceExt;

    use super::{
        BearerAuth, FixedProgramLoader, HttpConfig, ProgramLoader, build_active_router,
        build_router, build_router_with_auth, serve_with_shutdown,
    };

    const TASK_SERVICE: &str = include_str!("../../../../examples/tasks/service.ail");

    fn require<T>(result: AilResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap_or_else(|error| panic!("runtime failed: {error}"))
    }

    fn fixed_router(path: &std::path::Path, config: HttpConfig) -> Router {
        let loader: Arc<dyn ProgramLoader> =
            Arc::new(require(FixedProgramLoader::from_source(TASK_SERVICE)));
        let store = require(ail_service::FileKvStore::open(path));
        require(build_router(loader, store, config))
    }

    fn authenticated_router(path: &std::path::Path, token: &str) -> Router {
        let loader: Arc<dyn ProgramLoader> =
            Arc::new(require(FixedProgramLoader::from_source(TASK_SERVICE)));
        let store = require(ail_service::FileKvStore::open(path));
        let authentication = require(BearerAuth::new(token.to_owned()));
        require(build_router_with_auth(
            loader,
            store,
            HttpConfig::default(),
            Some(authentication),
        ))
    }

    async fn call_raw(router: Router, request: Request<Body>) -> Response {
        router
            .oneshot(request)
            .await
            .unwrap_or_else(|error| match error {})
    }

    async fn call(router: Router, request: Request<Body>) -> (StatusCode, JsonValue) {
        let response = call_raw(router, request).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap_or_else(|error| panic!("response body failed: {error}"));
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("response JSON failed: {error}"));
        (status, body)
    }

    fn request(method: &str, path: &str, body: Option<&JsonValue>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        let payload = match body {
            Some(value) => {
                builder = builder.header(CONTENT_TYPE, "application/json");
                Body::from(
                    serde_json::to_vec(value)
                        .unwrap_or_else(|error| panic!("request JSON failed: {error}")),
                )
            }
            None => Body::empty(),
        };
        builder
            .body(payload)
            .unwrap_or_else(|error| panic!("request build failed: {error}"))
    }

    #[test]
    fn task_crud_runs_through_http_and_survives_router_restart() {
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        runtime().block_on(async {
            let router = fixed_router(&store_path, HttpConfig::default());
            let create = json!({ "id": "http", "title": "through axum", "completed": false });
            let (status, _) = call(router, request("POST", "/tasks", Some(&create))).await;
            assert_eq!(status, StatusCode::CREATED);

            let restarted = fixed_router(&store_path, HttpConfig::default());
            let (status, body) = call(restarted, request("GET", "/tasks/http", None)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body.get("title").and_then(JsonValue::as_str),
                Some("through axum")
            );
        });
    }

    #[test]
    fn protocol_boundary_rejects_media_type_json_and_oversized_bodies() {
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        runtime().block_on(async {
            let router = fixed_router(&store_path, HttpConfig::default());
            let unsupported = Request::builder()
                .method("POST")
                .uri("/tasks")
                .header(CONTENT_TYPE, "text/plain")
                .body(Body::from("{}"))
                .unwrap_or_else(|error| panic!("request build failed: {error}"));
            let (status, body) = call(router.clone(), unsupported).await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
            assert_eq!(body["error"]["code"], "HTTP_UNSUPPORTED_MEDIA_TYPE");

            let invalid = Request::builder()
                .method("POST")
                .uri("/tasks")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap_or_else(|error| panic!("request build failed: {error}"));
            let (status, body) = call(router, invalid).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["code"], "HTTP_INVALID_JSON");

            let limited = fixed_router(
                &store_path,
                HttpConfig {
                    maximum_body_bytes: 8,
                    ..HttpConfig::default()
                },
            );
            let oversized = Request::builder()
                .method("POST")
                .uri("/tasks")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{\"too\":true}"))
                .unwrap_or_else(|error| panic!("request build failed: {error}"));
            let (status, body) = call(limited, oversized).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
            assert_eq!(body["error"]["code"], "HTTP_INVALID_CONTENT_LENGTH");
        });
    }

    #[test]
    fn bearer_authentication_request_ids_and_sensitive_header_filtering_are_host_owned() {
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        runtime().block_on(async {
            let router = authenticated_router(&store_path, "correct-horse-battery-staple");
            let unauthorized = call_raw(router.clone(), request("GET", "/tasks", None)).await;
            assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                unauthorized.headers().get(WWW_AUTHENTICATE),
                Some(&HeaderValue::from_static("Bearer"))
            );
            let first_id = unauthorized
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_else(|| panic!("unauthorized response must have a request ID"))
                .to_owned();
            assert!(first_id.starts_with("req-"));
            assert_eq!(first_id.len(), 36);

            let authorized = Request::builder()
                .method("GET")
                .uri("/tasks")
                .header(AUTHORIZATION, "bEaReR correct-horse-battery-staple")
                .header(COOKIE, "session=guest-must-not-see-this")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request build failed: {error}"));
            let authorized = call_raw(router, authorized).await;
            assert_eq!(authorized.status(), StatusCode::OK);
            let second_id = authorized
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_else(|| panic!("authorized response must have a request ID"));
            assert_ne!(first_id, second_id);
        });

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        headers.insert(COOKIE, HeaderValue::from_static("session=secret"));
        headers.insert("x-request-id", HeaderValue::from_static("client-spoof"));
        headers.insert("x-visible", HeaderValue::from_static("yes"));
        let parsed = require(super::parse_headers(&headers, &HttpConfig::default()));
        assert!(!parsed.contains_key("authorization"));
        assert!(!parsed.contains_key("cookie"));
        assert!(!parsed.contains_key("x-request-id"));
        assert!(parsed.contains_key("x-visible"));
    }

    #[test]
    fn internal_error_request_id_matches_the_response_header() {
        const FAILING_SERVICE: &str = r#"
            (program
              (name failing-service)
              (version 1)
              (capabilities)
              (route GET "/boom" boom)
              (def boom (fn (request) (get (map) "missing")))
              (export boom))
        "#;
        let temporary = TestDirectory::new();
        let store_path = temporary.path.join("store.json");
        let loader: Arc<dyn ProgramLoader> =
            Arc::new(require(FixedProgramLoader::from_source(FAILING_SERVICE)));
        let store = require(ail_service::FileKvStore::open(&store_path));
        let router = require(build_router(loader, store, HttpConfig::default()));
        runtime().block_on(async {
            let response = call_raw(router, request("GET", "/boom", None)).await;
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_else(|| panic!("internal response must have a request ID"))
                .to_owned();
            let bytes = to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap_or_else(|error| panic!("response body failed: {error}"));
            let body: JsonValue = serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("response JSON failed: {error}"));
            assert_eq!(body["error"]["details"]["requestId"], request_id);
        });
    }

    #[test]
    fn active_router_loads_a_test_gated_version_for_each_request() {
        let temporary = TestDirectory::new();
        let code_path = temporary.path.join("code");
        let data_path = temporary.path.join("data.json");
        let versions = VersionStore::new(&code_path);
        let report = json!({ "passed": true });
        let metadata = json!({});
        let hash = require(versions.register_candidate(CandidateRegistration {
            source: TASK_SERVICE,
            parent: None,
            provider: "http-test",
            provider_metadata: &metadata,
            report: &report,
            registered_at: 1,
        }));
        require(versions.promote(&hash, 2));

        runtime().block_on(async {
            let router = require(build_active_router(
                &code_path,
                &data_path,
                HttpConfig::default(),
            ));
            let (status, body) = call(router, request("GET", "/tasks", None)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, json!([]));
        });
    }

    #[test]
    fn serves_an_active_program_over_a_real_tcp_connection() {
        let temporary = TestDirectory::new();
        let code_path = temporary.path.join("code");
        let data_path = temporary.path.join("data.json");
        let versions = VersionStore::new(&code_path);
        let report = json!({ "passed": true });
        let metadata = json!({});
        let hash = require(versions.register_candidate(CandidateRegistration {
            source: TASK_SERVICE,
            parent: None,
            provider: "tcp-test",
            provider_metadata: &metadata,
            report: &report,
            registered_at: 1,
        }));
        require(versions.promote(&hash, 2));

        runtime().block_on(async {
            let router = require(build_active_router(
                &code_path,
                &data_path,
                HttpConfig::default(),
            ));
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap_or_else(|error| panic!("TCP bind failed: {error}"));
            let address = listener
                .local_addr()
                .unwrap_or_else(|error| panic!("listener address failed: {error}"));
            let (shutdown_sender, shutdown_receiver) = oneshot::channel();
            let server = tokio::spawn(async move {
                serve_with_shutdown(listener, router, async {
                    let _ignored = shutdown_receiver.await;
                })
                .await
            });

            let mut connection = TcpStream::connect(address)
                .await
                .unwrap_or_else(|error| panic!("TCP connect failed: {error}"));
            connection
                .write_all(b"GET /tasks HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await
                .unwrap_or_else(|error| panic!("HTTP write failed: {error}"));
            let mut response = Vec::new();
            connection
                .read_to_end(&mut response)
                .await
                .unwrap_or_else(|error| panic!("HTTP read failed: {error}"));
            let response = String::from_utf8(response)
                .unwrap_or_else(|error| panic!("HTTP response was not UTF-8: {error}"));
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
            assert!(response.ends_with("\r\n\r\n[]"));

            let _ignored = shutdown_sender.send(());
            let result = server
                .await
                .unwrap_or_else(|error| panic!("server task failed: {error}"));
            result.unwrap_or_else(|error| panic!("server failed: {error}"));
        });
    }

    #[test]
    fn percent_decoding_is_strict_and_encoded_slashes_are_rejected() {
        assert_eq!(
            super::decode_path("/tasks/%E4%B8%AD"),
            Ok("/tasks/中".to_owned())
        );
        assert!(super::decode_path("/tasks/%2F").is_err());
        assert!(super::decode_query("bad=%GG").is_err());
        assert_eq!(
            super::decode_query("q=hello+world")
                .ok()
                .and_then(|query| query.get("q").cloned()),
            Some(ail_runtime::Value::String("hello world".to_owned()))
        );
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
                .join(format!("ai-lang-rust-http-{}-{nonce}", std::process::id()));
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
