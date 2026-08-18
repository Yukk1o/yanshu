#![forbid(unsafe_code)]

use serde_json::{Value, json};

pub type AilResult<T> = Result<T, Diagnostic>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: Box<str>,
    pub details: Box<Value>,
    pub span: Option<Box<Span>>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into().into_boxed_str(),
            details: Box::new(details),
            span: None,
        }
    }

    #[must_use]
    pub fn simple(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message, json!({}))
    }

    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(Box::new(span));
        self
    }

    #[must_use]
    pub fn public_json(&self) -> Value {
        json!({
            "ok": false,
            "error": {
                "code": self.code,
                "message": self.message.as_ref(),
                "details": self.details.as_ref(),
            }
        })
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::Diagnostic;
    use serde_json::json;

    #[test]
    fn public_envelope_does_not_expose_host_fields() {
        let diagnostic = Diagnostic::new("TEST", "failed", json!({ "value": 1 }));
        assert_eq!(
            diagnostic.public_json(),
            json!({
                "ok": false,
                "error": {
                    "code": "TEST",
                    "message": "failed",
                    "details": { "value": 1 }
                }
            })
        );
    }
}
