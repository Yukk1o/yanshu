#![forbid(unsafe_code)]

use serde_json::{Map, Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{storage::MAXIMUM_JOURNAL_BYTES, validate_hash};

#[derive(Debug, Clone)]
pub(crate) enum PendingTransaction {
    Register {
        source: String,
        metadata: JsonValue,
        event: JsonValue,
    },
    Activate {
        from: Option<String>,
        to: String,
        event: JsonValue,
    },
}

impl PendingTransaction {
    pub(crate) fn to_bytes(&self) -> YanshuResult<Vec<u8>> {
        let document = match self {
            Self::Register {
                source,
                metadata,
                event,
            } => json!({
                "schemaVersion": 1,
                "operation": "register",
                "source": source,
                "metadata": metadata,
                "event": event,
            }),
            Self::Activate { from, to, event } => json!({
                "schemaVersion": 1,
                "operation": "activate",
                "from": from,
                "to": to,
                "event": event,
            }),
        };
        let mut bytes = serde_json::to_vec(&document).map_err(|_| invalid_journal())?;
        bytes.push(b'\n');
        enforce_size(bytes.len())?;
        Ok(bytes)
    }

    pub(crate) fn parse(bytes: &[u8]) -> YanshuResult<Self> {
        enforce_size(bytes.len())?;
        let document: JsonValue = serde_json::from_slice(bytes).map_err(|_| invalid_journal())?;
        let object = document.as_object().ok_or_else(invalid_journal)?;
        if object.get("schemaVersion").and_then(JsonValue::as_u64) != Some(1) {
            return Err(invalid_journal());
        }
        match object.get("operation").and_then(JsonValue::as_str) {
            Some("register")
                if exact_fields(
                    object,
                    &["schemaVersion", "operation", "source", "metadata", "event"],
                ) =>
            {
                let source = object
                    .get("source")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(invalid_journal)?
                    .to_owned();
                let metadata = object
                    .get("metadata")
                    .filter(|value| value.is_object())
                    .ok_or_else(invalid_journal)?
                    .clone();
                let event = object
                    .get("event")
                    .filter(|value| value.is_object())
                    .ok_or_else(invalid_journal)?
                    .clone();
                Ok(Self::Register {
                    source,
                    metadata,
                    event,
                })
            }
            Some("activate")
                if exact_fields(
                    object,
                    &["schemaVersion", "operation", "from", "to", "event"],
                ) =>
            {
                let from = optional_hash(object.get("from"))?;
                let to = required_hash(object.get("to"))?;
                let event = object
                    .get("event")
                    .filter(|value| value.is_object())
                    .ok_or_else(invalid_journal)?
                    .clone();
                Ok(Self::Activate { from, to, event })
            }
            _ => Err(invalid_journal()),
        }
    }
}

fn exact_fields(object: &Map<String, JsonValue>, fields: &[&str]) -> bool {
    object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
}

fn required_hash(value: Option<&JsonValue>) -> YanshuResult<String> {
    let hash = value
        .and_then(JsonValue::as_str)
        .ok_or_else(invalid_journal)?;
    validate_hash(hash).map_err(|_| invalid_journal())?;
    Ok(hash.to_owned())
}

fn optional_hash(value: Option<&JsonValue>) -> YanshuResult<Option<String>> {
    match value {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(hash)) => {
            validate_hash(hash).map_err(|_| invalid_journal())?;
            Ok(Some(hash.clone()))
        }
        _ => Err(invalid_journal()),
    }
}

fn enforce_size(length: usize) -> YanshuResult<()> {
    if u64::try_from(length).unwrap_or(u64::MAX) <= MAXIMUM_JOURNAL_BYTES {
        Ok(())
    } else {
        Err(Diagnostic::new(
            "VERSION_RECOVERY_LIMIT",
            "version recovery journal exceeds its byte limit",
            json!({ "maximum": MAXIMUM_JOURNAL_BYTES }),
        ))
    }
}

pub(crate) fn invalid_journal() -> Diagnostic {
    Diagnostic::simple(
        "VERSION_RECOVERY_INVALID",
        "version recovery journal is malformed or inconsistent with the store",
    )
}
