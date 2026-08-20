#![forbid(unsafe_code)]

use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_service::ServiceResponse;

pub(crate) fn service_response_to_http(
    response: ServiceResponse,
    head: bool,
    maximum_bytes: usize,
) -> YanshuResult<Response> {
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
        if is_forbidden_response_header(name.as_str()) {
            return Err(Diagnostic::new(
                "HTTP_RESPONSE_HEADER_FORBIDDEN",
                "guest response cannot control framing, connection, or authentication headers",
                json!({ "header": name.as_str() }),
            ));
        }
        let value = HeaderValue::try_from(value).map_err(|_| {
            Diagnostic::simple("HTTP_RESPONSE_HEADER", "response header value is invalid")
        })?;
        output.headers_mut().insert(name, value);
    }
    Ok(output)
}

fn is_forbidden_response_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "content-length"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "set-cookie"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "www-authenticate"
            | "x-request-id"
    )
}
