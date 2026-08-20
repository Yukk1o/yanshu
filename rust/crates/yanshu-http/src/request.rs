#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use axum::{
    body::to_bytes,
    extract::Request,
    http::{
        HeaderMap,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
};
use serde_json::{Value as JsonValue, json};
use tokio::time;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_runtime::{Value as GuestValue, json_to_value};
use yanshu_service::ServiceRequest;

use crate::HttpConfig;

/// Normalizes an untrusted Axum request into the bounded guest request shape.
///
/// This is the production post-HTTP-parser trust boundary and is public so
/// fuzzers and alternate host adapters can exercise exactly the same checks.
pub async fn normalize_http_request(
    request: Request,
    config: &HttpConfig,
) -> YanshuResult<ServiceRequest> {
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

pub(crate) fn parse_headers(
    headers: &HeaderMap,
    config: &HttpConfig,
) -> YanshuResult<BTreeMap<String, GuestValue>> {
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
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "cookie"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "x-api-key"
            | "x-auth-token"
            | "x-access-token"
            | "x-session-token"
            | "x-amz-security-token"
            | "x-goog-api-key"
            | "x-request-id"
    ) || name.contains("credential")
        || name.contains("secret")
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

pub(crate) fn decode_path(raw: &str) -> YanshuResult<String> {
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
        .collect::<YanshuResult<Vec<_>>>()?;
    Ok(format!("/{}", segments.join("/")))
}

pub(crate) fn decode_query(raw: &str) -> YanshuResult<BTreeMap<String, GuestValue>> {
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
