#![forbid(unsafe_code)]

use axum::http::{HeaderMap, header::AUTHORIZATION};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use zeroize::Zeroizing;

pub struct BearerAuth {
    token_digest: [u8; 32],
}

impl BearerAuth {
    pub fn new(token: String) -> YanshuResult<Self> {
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

    pub(crate) fn authorizes(&self, headers: &HeaderMap) -> bool {
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
