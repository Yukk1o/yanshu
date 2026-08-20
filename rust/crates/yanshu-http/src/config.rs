#![forbid(unsafe_code)]

use std::time::Duration;

use yanshu_diagnostic::{Diagnostic, YanshuResult};

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub maximum_target_bytes: usize,
    pub maximum_header_bytes: usize,
    pub maximum_headers: usize,
    pub maximum_body_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_concurrency: usize,
    pub header_read_timeout: Duration,
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
            header_read_timeout: Duration::from_secs(10),
            body_read_timeout: Duration::from_secs(10),
        }
    }
}

pub(crate) fn validate(config: &HttpConfig) -> YanshuResult<()> {
    if config.maximum_target_bytes == 0
        || config.maximum_header_bytes == 0
        || config.maximum_headers == 0
        || config.maximum_body_bytes == 0
        || config.maximum_response_bytes == 0
        || config.maximum_concurrency == 0
        || config.header_read_timeout.is_zero()
        || config.body_read_timeout.is_zero()
    {
        return Err(Diagnostic::simple(
            "HTTP_INVALID_CONFIG",
            "HTTP limits and timeouts must be positive",
        ));
    }
    Ok(())
}
