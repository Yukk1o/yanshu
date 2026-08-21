#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde_json::{Map, Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    storage::{MAXIMUM_EVENT_BYTES, MAXIMUM_EVENT_LOG_BYTES, MAXIMUM_EVENTS, sha256_hex},
    validate_hash,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionEvent {
    Registered {
        hash: String,
        parent: Option<String>,
        provider: String,
        at: u64,
    },
    Promoted {
        from: Option<String>,
        to: String,
        at: u64,
    },
    RolledBack {
        from: String,
        to: String,
        at: u64,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct StoredEvent {
    pub(crate) document: JsonValue,
    pub(crate) event: VersionEvent,
}

#[derive(Debug, Clone)]
pub(crate) struct EventLog {
    pub(crate) bytes: Vec<u8>,
    pub(crate) events: Vec<StoredEvent>,
    pub(crate) registered: BTreeSet<String>,
    pub(crate) active: Option<String>,
    head: Option<String>,
}

impl EventLog {
    pub(crate) fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            events: Vec::new(),
            registered: BTreeSet::new(),
            active: None,
            head: None,
        }
    }

    pub(crate) fn next_event(&self, mut base: JsonValue) -> YanshuResult<JsonValue> {
        {
            let object = base.as_object_mut().ok_or_else(invalid_events)?;
            if ["schemaVersion", "sequence", "previousHash", "eventHash"]
                .iter()
                .any(|field| object.contains_key(*field))
            {
                return Err(invalid_events());
            }
            object.insert("schemaVersion".to_owned(), json!(2));
            object.insert("sequence".to_owned(), json!(self.events.len() + 1));
            object.insert(
                "previousHash".to_owned(),
                self.head.clone().map_or(JsonValue::Null, JsonValue::String),
            );
        }
        let payload = serde_json::to_vec(&base).map_err(|_| invalid_events())?;
        base.as_object_mut()
            .ok_or_else(invalid_events)?
            .insert("eventHash".to_owned(), json!(sha256_hex(&payload)));
        let _verified = self.prospective(&base)?;
        Ok(base)
    }

    pub(crate) fn prospective(&self, event: &JsonValue) -> YanshuResult<Self> {
        let mut line = serde_json::to_vec(event).map_err(|_| invalid_events())?;
        if line.len() > MAXIMUM_EVENT_BYTES {
            return Err(event_limit());
        }
        line.push(b'\n');
        let total = self
            .bytes
            .len()
            .checked_add(line.len())
            .filter(|total| {
                u64::try_from(*total).is_ok_and(|value| value <= MAXIMUM_EVENT_LOG_BYTES)
            })
            .ok_or_else(event_limit)?;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&self.bytes);
        bytes.extend_from_slice(&line);
        parse_event_log(bytes)
    }

    pub(crate) fn is_last(&self, event: &JsonValue) -> bool {
        self.events
            .last()
            .is_some_and(|stored| stored.document == *event)
    }
}

pub(crate) fn parse_event_log(bytes: Vec<u8>) -> YanshuResult<EventLog> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_EVENT_LOG_BYTES {
        return Err(event_limit());
    }
    if bytes.is_empty() {
        return Ok(EventLog::empty());
    }
    if !bytes.ends_with(b"\n") {
        return Err(invalid_events());
    }

    let mut events = Vec::new();
    let mut registered = BTreeSet::new();
    let mut active = None;
    let mut chained = false;
    let mut head = None;
    let mut legacy_prefix_bytes = 0_usize;
    let mut consumed = 0_usize;

    for record in bytes.split_inclusive(|byte| *byte == b'\n') {
        consumed = consumed.checked_add(record.len()).ok_or_else(event_limit)?;
        if record.len() > MAXIMUM_EVENT_BYTES + 1 || events.len() >= MAXIMUM_EVENTS {
            return Err(event_limit());
        }
        let document: JsonValue = serde_json::from_slice(record).map_err(|_| invalid_events())?;
        let object = document.as_object().ok_or_else(invalid_events)?;
        let is_chained = object.contains_key("schemaVersion");
        if is_chained {
            if !chained {
                chained = true;
                if legacy_prefix_bytes > 0 {
                    head = Some(sha256_hex(&bytes[..legacy_prefix_bytes]));
                }
            }
            validate_chain(object, events.len() + 1, head.as_deref())?;
            head = object
                .get("eventHash")
                .and_then(JsonValue::as_str)
                .map(str::to_owned);
        } else {
            if chained {
                return Err(invalid_events());
            }
            legacy_prefix_bytes = consumed;
        }
        let event = parse_event(object, is_chained)?;
        apply_lifecycle(&event, &mut registered, &mut active)?;
        events.push(StoredEvent { document, event });
    }
    if !chained && legacy_prefix_bytes > 0 {
        head = Some(sha256_hex(&bytes[..legacy_prefix_bytes]));
    }
    Ok(EventLog {
        bytes,
        events,
        registered,
        active,
        head,
    })
}

fn validate_chain(
    object: &Map<String, JsonValue>,
    expected_sequence: usize,
    expected_previous: Option<&str>,
) -> YanshuResult<()> {
    if object.get("schemaVersion").and_then(JsonValue::as_u64) != Some(2)
        || object.get("sequence").and_then(JsonValue::as_u64)
            != u64::try_from(expected_sequence).ok()
    {
        return Err(invalid_events());
    }
    match (object.get("previousHash"), expected_previous) {
        (Some(JsonValue::Null), None) => {}
        (Some(JsonValue::String(actual)), Some(expected)) if actual == expected => {
            validate_hash(actual)?;
        }
        _ => return Err(invalid_events()),
    }
    let actual = object
        .get("eventHash")
        .and_then(JsonValue::as_str)
        .ok_or_else(invalid_events)?;
    validate_hash(actual)?;
    let mut payload = JsonValue::Object(object.clone());
    payload
        .as_object_mut()
        .ok_or_else(invalid_events)?
        .remove("eventHash");
    let bytes = serde_json::to_vec(&payload).map_err(|_| invalid_events())?;
    if sha256_hex(&bytes) != actual {
        return Err(invalid_events());
    }
    Ok(())
}

fn parse_event(object: &Map<String, JsonValue>, chained: bool) -> YanshuResult<VersionEvent> {
    let chain_fields = if chained {
        &["schemaVersion", "sequence", "previousHash", "eventHash"][..]
    } else {
        &[][..]
    };
    match object.get("event").and_then(JsonValue::as_str) {
        Some("registered")
            if exact_fields(
                object,
                &["event", "hash", "parent", "provider", "at"],
                chain_fields,
            ) =>
        {
            let hash = required_hash(object.get("hash"))?;
            let parent = optional_hash(object.get("parent"))?;
            let provider = object
                .get("provider")
                .and_then(JsonValue::as_str)
                .filter(|value| {
                    if chained {
                        !value.is_empty() && value.len() <= 256
                    } else {
                        value.len() <= MAXIMUM_EVENT_BYTES
                    }
                })
                .ok_or_else(invalid_events)?
                .to_owned();
            let at = object
                .get("at")
                .and_then(JsonValue::as_u64)
                .ok_or_else(invalid_events)?;
            Ok(VersionEvent::Registered {
                hash,
                parent,
                provider,
                at,
            })
        }
        Some("promoted") if exact_fields(object, &["event", "from", "to", "at"], chain_fields) => {
            Ok(VersionEvent::Promoted {
                from: optional_hash(object.get("from"))?,
                to: required_hash(object.get("to"))?,
                at: object
                    .get("at")
                    .and_then(JsonValue::as_u64)
                    .ok_or_else(invalid_events)?,
            })
        }
        Some("rolled-back")
            if exact_fields(object, &["event", "from", "to", "at"], chain_fields) =>
        {
            Ok(VersionEvent::RolledBack {
                from: required_hash(object.get("from"))?,
                to: required_hash(object.get("to"))?,
                at: object
                    .get("at")
                    .and_then(JsonValue::as_u64)
                    .ok_or_else(invalid_events)?,
            })
        }
        _ => Err(invalid_events()),
    }
}

fn exact_fields(
    object: &Map<String, JsonValue>,
    event_fields: &[&str],
    chain_fields: &[&str],
) -> bool {
    object.len() == event_fields.len() + chain_fields.len()
        && event_fields
            .iter()
            .chain(chain_fields)
            .all(|field| object.contains_key(*field))
}

fn apply_lifecycle(
    event: &VersionEvent,
    registered: &mut BTreeSet<String>,
    active: &mut Option<String>,
) -> YanshuResult<()> {
    match event {
        VersionEvent::Registered { hash, .. } => {
            if !registered.insert(hash.clone()) {
                return Err(invalid_events());
            }
        }
        VersionEvent::Promoted { from, to, .. } => {
            if active != from || !registered.contains(to) {
                return Err(invalid_events());
            }
            *active = Some(to.clone());
        }
        VersionEvent::RolledBack { from, to, .. } => {
            if active.as_deref() != Some(from) || !registered.contains(to) {
                return Err(invalid_events());
            }
            *active = Some(to.clone());
        }
    }
    Ok(())
}

fn required_hash(value: Option<&JsonValue>) -> YanshuResult<String> {
    let hash = value
        .and_then(JsonValue::as_str)
        .ok_or_else(invalid_events)?;
    validate_hash(hash)?;
    Ok(hash.to_owned())
}

fn optional_hash(value: Option<&JsonValue>) -> YanshuResult<Option<String>> {
    match value {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(hash)) => {
            validate_hash(hash)?;
            Ok(Some(hash.clone()))
        }
        _ => Err(invalid_events()),
    }
}

pub(crate) fn invalid_events() -> Diagnostic {
    Diagnostic::simple(
        "VERSION_INVALID_EVENTS",
        "version event log is malformed or violates its hash chain",
    )
}

fn event_limit() -> Diagnostic {
    Diagnostic::new(
        "VERSION_EVENT_LIMIT",
        "version event log exceeds its structural limit",
        json!({
            "maximumBytes": MAXIMUM_EVENT_LOG_BYTES,
            "maximumEvents": MAXIMUM_EVENTS,
            "maximumEventBytes": MAXIMUM_EVENT_BYTES,
        }),
    )
}
