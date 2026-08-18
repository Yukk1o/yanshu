use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
};

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::{Value as JsonValue, json};

use crate::ShadowComparison;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowObservation {
    pub timestamp_ms: u64,
    pub request_id: String,
    pub active_version: Option<String>,
    pub candidate_version: String,
    pub outcome: ShadowOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowOutcome {
    Compared(Box<ShadowComparison>),
    CandidateUnavailable { error_code: String },
    CapacitySkipped,
}

pub trait ShadowObservationSink: Send + Sync {
    fn record(&self, observation: &ShadowObservation) -> AilResult<()>;
}

#[derive(Debug)]
pub struct JsonlShadowObservationSink {
    file: Mutex<File>,
}

impl JsonlShadowObservationSink {
    pub fn open(path: impl AsRef<Path>) -> AilResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|_| open_failure(path))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| open_failure(path))?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl ShadowObservationSink for JsonlShadowObservationSink {
    fn record(&self, observation: &ShadowObservation) -> AilResult<()> {
        let mut bytes =
            serde_json::to_vec(&observation_json(observation)).map_err(|_| write_failure())?;
        if bytes.len() > 8191 {
            return Err(Diagnostic::simple(
                "SHADOW_OBSERVATION_TOO_LARGE",
                "shadow observation exceeded the record size limit",
            ));
        }
        bytes.push(b'\n');
        let mut file = self.file.lock().map_err(|_| write_failure())?;
        file.write_all(&bytes)
            .and_then(|()| file.flush())
            .map_err(|_| write_failure())
    }
}

fn observation_json(observation: &ShadowObservation) -> JsonValue {
    let (outcome, equivalent, differences, active, candidate, error_code) =
        match &observation.outcome {
            ShadowOutcome::Compared(comparison) => (
                "compared",
                Some(comparison.equivalent),
                json!(comparison.differences),
                summary_json(&comparison.active),
                summary_json(&comparison.candidate),
                JsonValue::Null,
            ),
            ShadowOutcome::CandidateUnavailable { error_code } => (
                "candidate-unavailable",
                None,
                json!([]),
                JsonValue::Null,
                JsonValue::Null,
                json!(error_code),
            ),
            ShadowOutcome::CapacitySkipped => (
                "capacity-skipped",
                None,
                json!([]),
                JsonValue::Null,
                JsonValue::Null,
                JsonValue::Null,
            ),
        };
    json!({
        "schemaVersion": 1,
        "timestampMs": observation.timestamp_ms,
        "requestId": observation.request_id,
        "activeVersion": observation.active_version,
        "candidateVersion": observation.candidate_version,
        "outcome": outcome,
        "equivalent": equivalent,
        "differences": differences,
        "active": active,
        "candidate": candidate,
        "errorCode": error_code,
    })
}

fn summary_json(summary: &crate::ExecutionSummary) -> JsonValue {
    json!({
        "status": summary.status,
        "handler": summary.handler,
        "errorCode": summary.error_code,
    })
}

fn open_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "SHADOW_OBSERVATION_OPEN_FAILURE",
        "shadow observation log could not be opened",
        json!({ "path": path.display().to_string() }),
    )
}

fn write_failure() -> Diagnostic {
    Diagnostic::simple(
        "SHADOW_OBSERVATION_WRITE_FAILURE",
        "shadow observation could not be appended",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        JsonlShadowObservationSink, ShadowObservation, ShadowObservationSink, ShadowOutcome,
    };

    #[test]
    fn unavailable_record_contains_only_bounded_metadata() {
        let path = temporary_path();
        let sink = JsonlShadowObservationSink::open(&path)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        sink.record(&ShadowObservation {
            timestamp_ms: 42,
            request_id: "req-safe".to_owned(),
            active_version: Some("a".repeat(64)),
            candidate_version: "b".repeat(64),
            outcome: ShadowOutcome::CandidateUnavailable {
                error_code: "VERSION_INTEGRITY_FAILURE".to_owned(),
            },
        })
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("observation read failed: {error}"));
        assert!(source.contains("candidate-unavailable"));
        assert!(!source.contains("message"));
        let _ignored = fs::remove_file(path);
    }

    fn temporary_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "ail-shadow-observation-{}-{nonce}.jsonl",
            std::process::id()
        ))
    }
}
