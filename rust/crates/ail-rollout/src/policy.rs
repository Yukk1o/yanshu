use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowPolicy {
    candidate_version: String,
    sample_percent: u8,
}

impl ShadowPolicy {
    pub fn new(candidate_version: impl Into<String>, sample_percent: u8) -> AilResult<Self> {
        let candidate_version = candidate_version.into();
        validate_version(&candidate_version)?;
        if sample_percent > 100 {
            return Err(Diagnostic::new(
                "SHADOW_INVALID_PERCENT",
                "shadow sample percent must be between 0 and 100",
                json!({ "samplePercent": sample_percent }),
            ));
        }
        Ok(Self {
            candidate_version,
            sample_percent,
        })
    }

    #[must_use]
    pub fn candidate_version(&self) -> &str {
        &self.candidate_version
    }

    #[must_use]
    pub fn sample_percent(&self) -> u8 {
        self.sample_percent
    }

    #[must_use]
    pub fn selects(&self, request_id: &str) -> bool {
        if self.sample_percent == 0 {
            return false;
        }
        if self.sample_percent == 100 {
            return true;
        }
        let digest = Sha256::digest(request_id.as_bytes());
        let bucket = u16::from_be_bytes([digest[0], digest[1]]) % 100;
        bucket < u16::from(self.sample_percent)
    }
}

fn validate_version(version: &str) -> AilResult<()> {
    if version.len() == 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(());
    }
    Err(Diagnostic::simple(
        "SHADOW_INVALID_VERSION",
        "shadow candidate version must be a 64-character lowercase content hash",
    ))
}

#[cfg(test)]
mod tests {
    use super::ShadowPolicy;

    fn require<T>(result: ail_diagnostic::AilResult<T>) -> T {
        result.unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
    }

    #[test]
    fn sampling_is_deterministic_and_honors_boundaries() {
        let hash = "a".repeat(64);
        let disabled = require(ShadowPolicy::new(&hash, 0));
        let enabled = require(ShadowPolicy::new(&hash, 100));
        assert!(!disabled.selects("req-fixed"));
        assert!(enabled.selects("req-fixed"));

        let partial = require(ShadowPolicy::new(hash, 37));
        assert_eq!(partial.selects("req-stable"), partial.selects("req-stable"));
    }

    #[test]
    fn invalid_versions_and_percentages_fail_closed() {
        let version_error = match ShadowPolicy::new("not-a-version", 1) {
            Ok(_) => panic!("invalid version must fail"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(version_error.code, "SHADOW_INVALID_VERSION");

        let percent_error = match ShadowPolicy::new("b".repeat(64), 101) {
            Ok(_) => panic!("invalid percentage must fail"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(percent_error.code, "SHADOW_INVALID_PERCENT");
    }
}
