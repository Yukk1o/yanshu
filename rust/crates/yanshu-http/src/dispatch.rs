#![forbid(unsafe_code)]

use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{
        HeaderName, HeaderValue, StatusCode,
        header::{CONTENT_TYPE, WWW_AUTHENTICATE},
    },
    response::Response,
};
use num_bigint::BigInt;
use serde_json::{Value as JsonValue, json};
use tokio::task;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_service::{DispatchResult, ServiceRequest, handle_file_service_request_with_id};

use crate::{
    normalize_http_request,
    observation::{RequestObservation, bounded_label, report_failure, timestamp_milliseconds},
    response::service_response_to_http,
    router::HttpState,
    shadow,
};

pub(crate) async fn dispatch(State(state): State<HttpState>, request: Request) -> Response {
    let started = Instant::now();
    let method = request.method().as_str().to_owned();
    let request_id = match generate_request_id() {
        Ok(request_id) => request_id,
        Err(diagnostic) => {
            return protocol_error_response(StatusCode::INTERNAL_SERVER_ERROR, &diagnostic, None);
        }
    };
    let mut outcome = dispatch_identified(state.clone(), request, &request_id).await;
    if let Ok(value) = HeaderValue::try_from(request_id.as_str()) {
        outcome
            .response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), value);
    }
    if let Some(observations) = &state.observations {
        let observation = RequestObservation {
            timestamp_ms: timestamp_milliseconds(),
            request_id: request_id.clone(),
            method: bounded_label(method, 32),
            status: outcome.response.status().as_u16(),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            handler: outcome.handler.map(|value| bounded_label(value, 256)),
            version: outcome.version,
            error_code: outcome.error_code,
        };
        if let Err(diagnostic) = observations.record(&observation) {
            report_failure(&request_id, &diagnostic);
        }
    }
    outcome.response
}

struct DispatchOutcome {
    response: Response,
    handler: Option<String>,
    version: Option<String>,
    error_code: Option<String>,
}

enum ExecutionOutcome {
    Completed {
        result: DispatchResult,
        version: Option<String>,
        shadow: Option<Box<shadow::ShadowJob>>,
    },
    Failed {
        diagnostic: Diagnostic,
        version: Option<String>,
    },
}

async fn dispatch_identified(
    state: HttpState,
    request: Request,
    request_id: &str,
) -> DispatchOutcome {
    if let Some(authentication) = &state.authentication
        && !authentication.authorizes(request.headers())
    {
        let mut outcome = protocol_outcome(
            StatusCode::UNAUTHORIZED,
            &Diagnostic::simple(
                "HTTP_AUTH_REQUIRED",
                "valid Bearer authentication is required",
            ),
            Some(request_id),
            None,
        );
        outcome
            .response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return outcome;
    }
    let Ok(_permit) = Arc::clone(&state.permits).try_acquire_owned() else {
        return protocol_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            &Diagnostic::simple("HTTP_BUSY", "server concurrency limit is exhausted"),
            Some(request_id),
            None,
        );
    };
    let method_is_head = request.method() == axum::http::Method::HEAD;
    let service_request = match normalize_http_request(request, &state.config).await {
        Ok(request) => request,
        Err(diagnostic) => {
            return protocol_outcome(
                diagnostic_status(&diagnostic),
                &diagnostic,
                Some(request_id),
                None,
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
        Ok(ExecutionOutcome::Completed {
            result,
            version,
            shadow,
        }) => {
            if let Some(shadow) = shadow {
                shadow::launch(shadow);
            }
            dispatch_response(
                result,
                method_is_head,
                state.config.maximum_response_bytes,
                request_id,
                version,
            )
        }
        Ok(ExecutionOutcome::Failed {
            diagnostic,
            version,
        }) => protocol_outcome(
            diagnostic_status(&diagnostic),
            &diagnostic,
            Some(request_id),
            version,
        ),
        Err(_) => protocol_outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            &Diagnostic::simple(
                "HTTP_WORKER_FAILURE",
                "request worker could not be completed",
            ),
            Some(request_id),
            None,
        ),
    }
}

fn execute_request(
    state: &HttpState,
    request: &ServiceRequest,
    request_id: &str,
) -> ExecutionOutcome {
    let loaded = match state.loader.load() {
        Ok(loaded) => loaded,
        Err(_) => {
            return ExecutionOutcome::Failed {
                diagnostic: Diagnostic::simple(
                    "HTTP_SERVICE_UNAVAILABLE",
                    "service program is unavailable",
                ),
                version: None,
            };
        }
    };
    let shadow_admission = state
        .shadow
        .as_ref()
        .map_or(shadow::Admission::NotSelected, |controls| {
            controls.admit(request_id)
        });
    shadow_admission.record_capacity_skip(
        timestamp_milliseconds(),
        request_id,
        loaded.version.clone(),
    );
    let mut store = match state.store.lock() {
        Ok(store) => store,
        Err(_) => {
            return ExecutionOutcome::Failed {
                diagnostic: Diagnostic::simple(
                    "HTTP_STORE_UNAVAILABLE",
                    "service data store is unavailable",
                ),
                version: loaded.version,
            };
        }
    };
    let shadow_store = shadow_admission.is_admitted().then(|| store.snapshot());
    let clock_milliseconds = current_milliseconds();
    let result = handle_file_service_request_with_id(
        &loaded.program,
        request,
        &mut store,
        &clock_milliseconds,
        request_id,
    );
    let shadow = shadow_admission.into_job(
        shadow_store,
        shadow::JobContext {
            request: request.clone(),
            request_id: request_id.to_owned(),
            clock_milliseconds,
            active_version: loaded.version.clone(),
            active_result: result.clone(),
            timestamp_ms: timestamp_milliseconds(),
        },
    );
    ExecutionOutcome::Completed {
        result,
        version: loaded.version,
        shadow,
    }
}

fn dispatch_response(
    result: DispatchResult,
    head: bool,
    maximum_bytes: usize,
    request_id: &str,
    version: Option<String>,
) -> DispatchOutcome {
    let handler = result.handler;
    let error_code = result
        .diagnostic
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned);
    match service_response_to_http(result.response, head, maximum_bytes) {
        Ok(response) => DispatchOutcome {
            response,
            handler,
            version,
            error_code,
        },
        Err(diagnostic) => protocol_outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            &diagnostic,
            Some(request_id),
            version,
        ),
    }
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

fn protocol_outcome(
    status: StatusCode,
    diagnostic: &Diagnostic,
    request_id: Option<&str>,
    version: Option<String>,
) -> DispatchOutcome {
    DispatchOutcome {
        response: protocol_error_response(status, diagnostic, request_id),
        handler: None,
        version,
        error_code: Some(diagnostic.code.to_owned()),
    }
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

fn generate_request_id() -> YanshuResult<String> {
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

fn current_milliseconds() -> BigInt {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    BigInt::from(milliseconds)
}
