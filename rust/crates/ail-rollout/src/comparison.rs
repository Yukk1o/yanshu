use std::collections::BTreeMap;

use ail_service::DispatchResult;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub status: u16,
    pub handler: Option<String>,
    pub error_code: Option<String>,
    headers_sha256: String,
    body_sha256: String,
}

impl ExecutionSummary {
    #[must_use]
    pub fn from_dispatch(result: &DispatchResult) -> Self {
        Self {
            status: result.response.status,
            handler: result
                .handler
                .as_ref()
                .map(|value| bounded_label(value, 256)),
            error_code: result
                .diagnostic
                .as_ref()
                .and_then(|value| value.pointer("/error/code"))
                .and_then(JsonValue::as_str)
                .map(|value| bounded_label(value, 128)),
            headers_sha256: hash_headers(&result.response.headers),
            body_sha256: hash_json(&result.response.body),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowComparison {
    pub equivalent: bool,
    pub differences: Vec<&'static str>,
    pub active: ExecutionSummary,
    pub candidate: ExecutionSummary,
}

impl ShadowComparison {
    #[must_use]
    pub fn compare(active: &DispatchResult, candidate: &DispatchResult) -> Self {
        let active = ExecutionSummary::from_dispatch(active);
        let candidate = ExecutionSummary::from_dispatch(candidate);
        let mut differences = Vec::new();
        if active.status != candidate.status {
            differences.push("status");
        }
        if active.handler != candidate.handler {
            differences.push("handler");
        }
        if active.error_code != candidate.error_code {
            differences.push("error-code");
        }
        if active.headers_sha256 != candidate.headers_sha256 {
            differences.push("headers");
        }
        if active.body_sha256 != candidate.body_sha256 {
            differences.push("body");
        }
        Self {
            equivalent: differences.is_empty(),
            differences,
            active,
            candidate,
        }
    }
}

fn hash_headers(headers: &BTreeMap<String, String>) -> String {
    let bytes = serde_json::to_vec(headers).unwrap_or_default();
    hash_bytes(&bytes)
}

fn hash_json(value: &JsonValue) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn bounded_label(value: &str, maximum_characters: usize) -> String {
    value.chars().take(maximum_characters).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ail_service::{DispatchResult, ServiceResponse};
    use serde_json::json;

    use super::ShadowComparison;

    fn result(status: u16, secret: &str) -> DispatchResult {
        DispatchResult {
            response: ServiceResponse {
                status,
                headers: BTreeMap::from([("x-result".to_owned(), secret.to_owned())]),
                body: json!({ "secret": secret }),
            },
            diagnostic: None,
            handler: Some("handler".to_owned()),
        }
    }

    #[test]
    fn comparison_reports_categories_without_retaining_payloads() {
        let comparison = ShadowComparison::compare(
            &result(200, "active-secret"),
            &result(201, "candidate-secret"),
        );
        assert_eq!(comparison.differences, vec!["status", "headers", "body"]);
        assert!(!comparison.equivalent);
        let debug = format!("{comparison:?}");
        assert!(!debug.contains("active-secret"));
        assert!(!debug.contains("candidate-secret"));
    }
}
