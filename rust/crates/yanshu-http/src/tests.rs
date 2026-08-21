#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
use yanshu_diagnostic::YanshuResult;
use yanshu_rollout::{
    JsonlShadowObservationSink, ShadowObservationSink, ShadowPolicy, ShadowRuntime,
};
use yanshu_store::{CandidateRegistration, VersionStore};

use super::{
    BearerAuth, FixedProgramLoader, HttpConfig, JsonlObservationSink, ObservationSink,
    ProgramLoader, ShadowControls, build_active_router, build_active_router_with_controls,
    build_active_router_with_runtime_controls, build_router, build_router_with_auth,
    build_router_with_controls, serve_with_shutdown,
};

const TASK_SERVICE: &str = include_str!("../../../../examples/tasks/service.yan");

fn require<T>(result: YanshuResult<T>) -> T {
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
    let store = require(yanshu_service::FileKvStore::open(path));
    require(build_router(loader, store, config))
}

fn authenticated_router(path: &std::path::Path, token: &str) -> Router {
    let loader: Arc<dyn ProgramLoader> =
        Arc::new(require(FixedProgramLoader::from_source(TASK_SERVICE)));
    let store = require(yanshu_service::FileKvStore::open(path));
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

async fn wait_for_nonempty_file(path: &std::path::Path) -> String {
    for _attempt in 0..200 {
        if let Ok(source) = fs::read_to_string(path)
            && !source.trim().is_empty()
        {
            return source;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {}", path.display());
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
    headers.insert("x-auth-token", HeaderValue::from_static("alternate-secret"));
    headers.insert("x-visible", HeaderValue::from_static("yes"));
    let parsed = require(super::request::parse_headers(
        &headers,
        &HttpConfig::default(),
    ));
    assert!(!parsed.contains_key("authorization"));
    assert!(!parsed.contains_key("cookie"));
    assert!(!parsed.contains_key("x-request-id"));
    assert!(!parsed.contains_key("x-auth-token"));
    assert!(parsed.contains_key("x-visible"));

    let framing = match super::response::service_response_to_http(
        yanshu_service::ServiceResponse {
            status: 200,
            headers: BTreeMap::from([("transfer-encoding".to_owned(), "chunked".to_owned())]),
            body: json!({ "ok": true }),
        },
        false,
        1024,
    ) {
        Err(diagnostic) => diagnostic,
        Ok(_) => panic!("guest framing header must be rejected"),
    };
    assert_eq!(framing.code, "HTTP_RESPONSE_HEADER_FORBIDDEN");
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
    let observation_path = temporary.path.join("observations.jsonl");
    let loader: Arc<dyn ProgramLoader> =
        Arc::new(require(FixedProgramLoader::from_source(FAILING_SERVICE)));
    let store = require(yanshu_service::FileKvStore::open(&store_path));
    let observations: Arc<dyn ObservationSink> =
        Arc::new(require(JsonlObservationSink::open(&observation_path)));
    let router = require(build_router_with_controls(
        loader,
        store,
        HttpConfig::default(),
        None,
        Some(observations),
    ));
    let request_id = runtime().block_on(async {
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
        request_id
    });
    let source = fs::read_to_string(&observation_path)
        .unwrap_or_else(|error| panic!("observation read failed: {error}"));
    let observation: JsonValue = serde_json::from_str(source.trim())
        .unwrap_or_else(|error| panic!("observation JSON failed: {error}"));
    assert_eq!(observation["requestId"], request_id);
    assert_eq!(observation["status"], 500);
    assert_eq!(observation["handler"], "boom");
    assert!(observation["errorCode"].as_str().is_some());
    assert!(observation["version"].is_null());
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
        let config = HttpConfig::default();
        let router = require(build_active_router(&code_path, &data_path, config.clone()));
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
        let config = HttpConfig::default();
        let router = require(build_active_router(&code_path, &data_path, config.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("TCP bind failed: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("listener address failed: {error}"));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve_with_shutdown(listener, router, &config, async {
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
fn closes_connections_that_do_not_finish_request_headers_before_the_deadline() {
    let temporary = TestDirectory::new();
    let config = HttpConfig {
        header_read_timeout: Duration::from_millis(50),
        ..HttpConfig::default()
    };

    runtime().block_on(async {
        let router = fixed_router(
            &temporary.path.join("slow-header-data.json"),
            config.clone(),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("TCP bind failed: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("listener address failed: {error}"));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            serve_with_shutdown(listener, router, &config, async {
                let _ignored = shutdown_receiver.await;
            })
            .await
        });

        let mut connection = TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("TCP connect failed: {error}"));
        connection
            .write_all(b"GET /tasks HTTP/1.1\r\nHost:")
            .await
            .unwrap_or_else(|error| panic!("partial header write failed: {error}"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut byte = [0_u8; 1];
        match tokio::time::timeout(Duration::from_secs(2), connection.read(&mut byte)).await {
            Ok(Ok(0) | Err(_)) => {}
            Ok(Ok(_)) => panic!("slow header connection unexpectedly produced a response"),
            Err(_) => panic!("slow header connection remained open past its deadline"),
        }

        let _ignored = shutdown_sender.send(());
        let result = server
            .await
            .unwrap_or_else(|error| panic!("server task failed: {error}"));
        result.unwrap_or_else(|error| panic!("server failed: {error}"));
    });
}

#[test]
fn jsonl_observations_append_across_restart_and_exclude_request_data() {
    let temporary = TestDirectory::new();
    let code_path = temporary.path.join("code");
    let data_path = temporary.path.join("data.json");
    let observation_path = temporary.path.join("requests.jsonl");
    let versions = VersionStore::new(&code_path);
    let report = json!({ "passed": true });
    let metadata = json!({});
    let hash = require(versions.register_candidate(CandidateRegistration {
        source: TASK_SERVICE,
        parent: None,
        provider: "observation-test",
        provider_metadata: &metadata,
        report: &report,
        registered_at: 1,
    }));
    require(versions.promote(&hash, 2));

    runtime().block_on(async {
        let observations: Arc<dyn ObservationSink> =
            Arc::new(require(JsonlObservationSink::open(&observation_path)));
        let router = require(build_active_router_with_controls(
            &code_path,
            &data_path,
            HttpConfig::default(),
            Some(require(BearerAuth::new("token-must-not-appear".to_owned()))),
            Some(observations),
        ));
        let body = json!({
            "id": "body-id-must-not-appear",
            "title": "body-title-must-not-appear",
            "completed": false
        });
        let authorized = Request::builder()
            .method("POST")
            .uri("/tasks?query-value-must-not-appear=1")
            .header(AUTHORIZATION, "Bearer token-must-not-appear")
            .header(COOKIE, "cookie-must-not-appear")
            .header("x-api-key", "api-key-must-not-appear")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap_or_else(
                |error| panic!("request JSON failed: {error}"),
            )))
            .unwrap_or_else(|error| panic!("request build failed: {error}"));
        let response = call_raw(router, authorized).await;
        assert_eq!(response.status(), StatusCode::CREATED);

        let observations: Arc<dyn ObservationSink> =
            Arc::new(require(JsonlObservationSink::open(&observation_path)));
        let restarted = require(build_active_router_with_controls(
            &code_path,
            &data_path,
            HttpConfig::default(),
            Some(require(BearerAuth::new("token-must-not-appear".to_owned()))),
            Some(observations),
        ));
        let response = call_raw(restarted, request("GET", "/tasks", None)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    });

    let source = fs::read_to_string(&observation_path)
        .unwrap_or_else(|error| panic!("observation read failed: {error}"));
    for secret in [
        "token-must-not-appear",
        "cookie-must-not-appear",
        "api-key-must-not-appear",
        "query-value-must-not-appear",
        "body-id-must-not-appear",
        "body-title-must-not-appear",
        "/tasks",
    ] {
        assert!(!source.contains(secret), "observation leaked {secret}");
    }
    let observations = source
        .lines()
        .map(|line| {
            serde_json::from_str::<JsonValue>(line)
                .unwrap_or_else(|error| panic!("observation JSON failed: {error}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0]["schemaVersion"], 1);
    assert_eq!(observations[0]["method"], "POST");
    assert_eq!(observations[0]["status"], 201);
    assert_eq!(observations[0]["handler"], "create-task");
    assert_eq!(observations[0]["version"], hash);
    assert!(observations[0]["errorCode"].is_null());
    assert_eq!(
        observations[0].as_object().map(serde_json::Map::len),
        Some(9)
    );
    assert_eq!(observations[1]["status"], 401);
    assert_eq!(observations[1]["errorCode"], "HTTP_AUTH_REQUIRED");
    assert!(observations[1]["version"].is_null());
}

#[test]
fn shadow_candidate_uses_the_pre_request_snapshot_without_persisting_side_effects() {
    let temporary = TestDirectory::new();
    let code_path = temporary.path.join("code");
    let data_path = temporary.path.join("data.json");
    let shadow_path = temporary.path.join("shadow.jsonl");
    let versions = VersionStore::new(&code_path);
    let report = json!({ "passed": true });
    let metadata = json!({});
    let active_hash = require(versions.register_candidate(CandidateRegistration {
        source: TASK_SERVICE,
        parent: None,
        provider: "shadow-active-test",
        provider_metadata: &metadata,
        report: &report,
        registered_at: 1,
    }));
    require(versions.promote(&active_hash, 2));
    let candidate_source = TASK_SERVICE
            .replacen(
                "(do (kv-put key task)",
                "(do (kv-put \"shadow/side-effect\" (map \"secret\" \"shadow-kv-secret\"))\n                        (kv-put key task)",
                1,
            )
            .replacen(
                "(api-response 201 task)",
                "(api-response 202 (map \"shadowSecret\" \"shadow-response-secret\"))",
                1,
            );
    let candidate_hash = require(versions.register_candidate(CandidateRegistration {
        source: &candidate_source,
        parent: Some(&active_hash),
        provider: "shadow-candidate-test",
        provider_metadata: &metadata,
        report: &report,
        registered_at: 3,
    }));
    let shadow_sink: Arc<dyn ShadowObservationSink> =
        Arc::new(require(JsonlShadowObservationSink::open(&shadow_path)));
    let shadow_runtime = Arc::new(ShadowRuntime::new(
        &code_path,
        require(ShadowPolicy::new(&candidate_hash, 100)),
        shadow_sink,
    ));
    let shadow = require(ShadowControls::new(shadow_runtime, 1));

    runtime().block_on(async {
        let router = require(build_active_router_with_runtime_controls(
            &code_path,
            &data_path,
            HttpConfig::default(),
            None,
            None,
            Some(shadow),
        ));
        let body = json!({
            "id": "active-only-id",
            "title": "request-body-secret",
            "completed": false
        });
        let (status, response) = call(router, request("POST", "/tasks", Some(&body))).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(response["id"], "active-only-id");

        let source = wait_for_nonempty_file(&shadow_path).await;
        for secret in [
            "request-body-secret",
            "shadow-kv-secret",
            "shadow-response-secret",
        ] {
            assert!(
                !source.contains(secret),
                "shadow observation leaked {secret}"
            );
        }
        let observation: JsonValue = serde_json::from_str(source.trim())
            .unwrap_or_else(|error| panic!("shadow observation JSON failed: {error}"));
        assert_eq!(observation["activeVersion"], active_hash);
        assert_eq!(observation["candidateVersion"], candidate_hash);
        assert_eq!(observation["outcome"], "compared");
        assert_eq!(observation["active"]["status"], 201);
        assert_eq!(observation["candidate"]["status"], 202);
        assert_eq!(observation["differences"], json!(["status", "body"]));
        assert_eq!(
            observation["active"].as_object().map(serde_json::Map::len),
            Some(3)
        );
        assert!(observation["active"].get("bodySha256").is_none());
        assert!(observation["candidate"].get("headersSha256").is_none());
    });

    let persisted = fs::read_to_string(&data_path)
        .unwrap_or_else(|error| panic!("data store read failed: {error}"));
    assert!(persisted.contains("active-only-id"));
    assert!(!persisted.contains("shadow/side-effect"));
    assert!(!persisted.contains("shadow-kv-secret"));
}

#[test]
fn runtime_candidate_tampering_is_observed_without_changing_the_primary_response() {
    let temporary = TestDirectory::new();
    let code_path = temporary.path.join("code");
    let data_path = temporary.path.join("data.json");
    let shadow_path = temporary.path.join("shadow.jsonl");
    let versions = VersionStore::new(&code_path);
    let report = json!({ "passed": true });
    let metadata = json!({});
    let active_hash = require(versions.register_candidate(CandidateRegistration {
        source: TASK_SERVICE,
        parent: None,
        provider: "tamper-active-test",
        provider_metadata: &metadata,
        report: &report,
        registered_at: 1,
    }));
    require(versions.promote(&active_hash, 2));
    let candidate_source = format!("{TASK_SERVICE}\n");
    let candidate_hash = require(versions.register_candidate(CandidateRegistration {
        source: &candidate_source,
        parent: Some(&active_hash),
        provider: "tamper-candidate-test",
        provider_metadata: &metadata,
        report: &report,
        registered_at: 3,
    }));
    let shadow_sink: Arc<dyn ShadowObservationSink> =
        Arc::new(require(JsonlShadowObservationSink::open(&shadow_path)));
    let shadow_runtime = Arc::new(ShadowRuntime::new(
        &code_path,
        require(ShadowPolicy::new(&candidate_hash, 100)),
        shadow_sink,
    ));
    let shadow = require(ShadowControls::new(shadow_runtime, 1));
    fs::write(
        code_path
            .join("versions")
            .join(format!("{candidate_hash}.yan")),
        "tampered",
    )
    .unwrap_or_else(|error| panic!("candidate tamper fixture failed: {error}"));

    runtime().block_on(async {
        let router = require(build_active_router_with_runtime_controls(
            &code_path,
            &data_path,
            HttpConfig::default(),
            None,
            None,
            Some(shadow),
        ));
        let (status, body) = call(router, request("GET", "/tasks", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!([]));

        let source = wait_for_nonempty_file(&shadow_path).await;
        let observation: JsonValue = serde_json::from_str(source.trim())
            .unwrap_or_else(|error| panic!("shadow observation JSON failed: {error}"));
        assert_eq!(observation["outcome"], "candidate-unavailable");
        assert_eq!(observation["errorCode"], "VERSION_INTEGRITY_FAILURE");
        assert_eq!(observation["activeVersion"], active_hash);
        assert_eq!(observation["candidateVersion"], candidate_hash);
    });
}

#[test]
fn percent_decoding_is_strict_and_encoded_slashes_are_rejected() {
    assert_eq!(
        super::request::decode_path("/tasks/%E4%B8%AD"),
        Ok("/tasks/中".to_owned())
    );
    assert!(super::request::decode_path("/tasks/%2F").is_err());
    assert!(super::request::decode_query("bad=%GG").is_err());
    assert_eq!(
        super::request::decode_query("q=hello+world")
            .ok()
            .and_then(|query| query.get("q").cloned()),
        Some(yanshu_runtime::Value::String("hello world".to_owned()))
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
        let path =
            std::env::temp_dir().join(format!("ai-lang-rust-http-{}-{nonce}", std::process::id()));
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
