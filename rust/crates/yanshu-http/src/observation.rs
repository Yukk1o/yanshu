#![forbid(unsafe_code)]

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, to_vec};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestObservation {
    pub timestamp_ms: u64,
    pub request_id: String,
    pub method: String,
    pub status: u16,
    pub duration_ms: u64,
    pub handler: Option<String>,
    pub version: Option<String>,
    pub error_code: Option<String>,
}

pub trait ObservationSink: Send + Sync {
    fn record(&self, observation: &RequestObservation) -> YanshuResult<()>;
}

#[derive(Debug)]
pub struct JsonlObservationSink {
    file: Mutex<File>,
}

impl JsonlObservationSink {
    pub fn open(path: impl AsRef<Path>) -> YanshuResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|_| observation_open_failure(path))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| observation_open_failure(path))?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl ObservationSink for JsonlObservationSink {
    fn record(&self, observation: &RequestObservation) -> YanshuResult<()> {
        let mut bytes = to_vec(&json!({
            "schemaVersion": 1,
            "timestampMs": observation.timestamp_ms,
            "requestId": observation.request_id,
            "method": observation.method,
            "status": observation.status,
            "durationMs": observation.duration_ms,
            "handler": observation.handler,
            "version": observation.version,
            "errorCode": observation.error_code,
        }))
        .map_err(|_| observation_write_failure())?;
        if bytes.len() > 4095 {
            return Err(Diagnostic::simple(
                "OBSERVATION_TOO_LARGE",
                "request observation exceeded the record size limit",
            ));
        }
        bytes.push(b'\n');
        let mut file = self.file.lock().map_err(|_| observation_write_failure())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| observation_write_failure())
    }
}

pub(crate) fn timestamp_milliseconds() -> u64 {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(milliseconds).unwrap_or(u64::MAX)
}

pub(crate) fn bounded_label(value: String, maximum_characters: usize) -> String {
    value.chars().take(maximum_characters).collect()
}

pub(crate) fn report_failure(request_id: &str, diagnostic: &Diagnostic) {
    eprintln!(
        "{}",
        json!({
            "ok": false,
            "event": "observation-write-failed",
            "requestId": request_id,
            "errorCode": diagnostic.code,
        })
    );
}

fn observation_open_failure(path: &Path) -> Diagnostic {
    Diagnostic::new(
        "OBSERVATION_OPEN_FAILURE",
        "request observation log could not be opened",
        json!({ "path": path.display().to_string() }),
    )
}

fn observation_write_failure() -> Diagnostic {
    Diagnostic::simple(
        "OBSERVATION_WRITE_FAILURE",
        "request observation could not be appended",
    )
}
