#![forbid(unsafe_code)]

use serde_json::Value as JsonValue;
use yanshu_diagnostic::YanshuResult;
use yanshu_syntax::load_program_source;

use crate::{
    events::{EventLog, VersionEvent},
    integrity_failure,
    metadata::{report_passed, validate_metadata, write_json_atomically},
    metadata_parent,
    storage::{
        MAXIMUM_EVENT_LOG_BYTES, MAXIMUM_JOURNAL_BYTES, MAXIMUM_METADATA_BYTES,
        MAXIMUM_SOURCE_BYTES, atomic_replace, read_bounded, remove_durably,
    },
    store::VersionStore,
    transaction::{PendingTransaction, invalid_journal},
    validate_hash, write_failure,
};

const JOURNAL_FILE: &str = ".yanshu-store.pending.json";

impl VersionStore {
    pub(crate) fn commit_transaction(&self, transaction: &PendingTransaction) -> YanshuResult<()> {
        let path = self.root().join(JOURNAL_FILE);
        if path.exists() {
            return Err(invalid_journal());
        }
        let bytes = transaction.to_bytes()?;
        atomic_replace(&path, &bytes).map_err(|_| write_failure(&path))?;
        self.inject_failure(1)?;
        self.apply_transaction(transaction)?;
        remove_durably(&path).map_err(|_| write_failure(&path))
    }

    pub(crate) fn recover_unlocked(&self) -> YanshuResult<()> {
        let path = self.root().join(JOURNAL_FILE);
        if !path.exists() {
            return Ok(());
        }
        let bytes = read_bounded(&path, MAXIMUM_JOURNAL_BYTES).map_err(|_| invalid_journal())?;
        let transaction = PendingTransaction::parse(&bytes)?;
        self.apply_transaction(&transaction)?;
        remove_durably(&path).map_err(|_| write_failure(&path))
    }

    fn apply_transaction(&self, transaction: &PendingTransaction) -> YanshuResult<()> {
        match transaction {
            PendingTransaction::Register {
                source,
                metadata,
                event,
            } => self.apply_registration(source, metadata, event),
            PendingTransaction::Activate { from, to, event } => {
                self.apply_activation(from.as_deref(), to, event)
            }
        }
    }

    fn apply_registration(
        &self,
        source: &str,
        metadata: &JsonValue,
        event: &JsonValue,
    ) -> YanshuResult<()> {
        if u64::try_from(source.len()).unwrap_or(u64::MAX) > MAXIMUM_SOURCE_BYTES {
            return Err(invalid_journal());
        }
        let program = load_program_source(source).map_err(|_| invalid_journal())?;
        let hash = validate_metadata(metadata, source, &program)?;
        let log = self.load_event_log_unlocked()?;
        self.require_active_matches(&log)?;
        let committed = log.is_last(event);
        let next = if committed {
            log.clone()
        } else {
            log.prospective(event).map_err(|_| invalid_journal())?
        };
        let stored = next.events.last().ok_or_else(invalid_journal)?;
        match &stored.event {
            VersionEvent::Registered {
                hash: event_hash,
                parent,
                provider,
                at,
            } if event_hash == &hash
                && *parent == metadata_parent(metadata)?
                && metadata.get("provider").and_then(JsonValue::as_str)
                    == Some(provider.as_str())
                && metadata.get("registeredAt").and_then(JsonValue::as_u64) == Some(*at) => {}
            _ => return Err(invalid_journal()),
        }

        self.ensure_source(&hash, source)?;
        self.inject_failure(2)?;
        self.ensure_metadata(&hash, metadata)?;
        self.inject_failure(3)?;
        if !committed {
            self.write_event_log(&next)?;
        }
        self.inject_failure(4)
    }

    fn apply_activation(
        &self,
        from: Option<&str>,
        to: &str,
        event: &JsonValue,
    ) -> YanshuResult<()> {
        validate_hash(to).map_err(|_| invalid_journal())?;
        let log = self.load_event_log_unlocked()?;
        let physical = self.active_hash_unlocked()?;
        let committed = log.is_last(event);
        let next = if committed {
            log.clone()
        } else {
            log.prospective(event).map_err(|_| invalid_journal())?
        };
        let stored = next.events.last().ok_or_else(invalid_journal)?;
        self.validate_activation_event(&stored.event, from, to, &log)?;

        if committed {
            if log.active.as_deref() != Some(to) || physical.as_deref() != Some(to) {
                return Err(invalid_journal());
            }
        } else {
            if log.active.as_deref() != from {
                return Err(invalid_journal());
            }
            if physical.as_deref() == from {
                self.write_active_pointer(to)?;
            } else if physical.as_deref() != Some(to) {
                return Err(invalid_journal());
            }
            self.inject_failure(2)?;
            self.write_event_log(&next)?;
        }
        self.inject_failure(3)
    }

    fn validate_activation_event(
        &self,
        event: &VersionEvent,
        from: Option<&str>,
        to: &str,
        log: &EventLog,
    ) -> YanshuResult<()> {
        match event {
            VersionEvent::Promoted {
                from: event_from,
                to: event_to,
                ..
            } if event_from.as_deref() == from && event_to == to => {
                let metadata = self.version_metadata_unlocked(to)?;
                self.version_source_unlocked(to)?;
                if !log.registered.contains(to)
                    || metadata_parent(&metadata)?.as_deref() != from
                    || !report_passed(&metadata)
                {
                    return Err(invalid_journal());
                }
            }
            VersionEvent::RolledBack {
                from: event_from,
                to: event_to,
                ..
            } if Some(event_from.as_str()) == from && event_to == to => {
                let from_metadata = self.version_metadata_unlocked(event_from)?;
                self.version_metadata_unlocked(to)?;
                self.version_source_unlocked(to)?;
                if !log.registered.contains(to)
                    || metadata_parent(&from_metadata)?.as_deref() != Some(to)
                {
                    return Err(invalid_journal());
                }
            }
            _ => return Err(invalid_journal()),
        }
        Ok(())
    }

    fn ensure_source(&self, hash: &str, source: &str) -> YanshuResult<()> {
        let path = self.root().join("versions").join(format!("{hash}.yan"));
        if path.exists() {
            if self.version_source_unlocked(hash)? != source {
                return Err(integrity_failure(hash));
            }
            Ok(())
        } else {
            atomic_replace(&path, source.as_bytes()).map_err(|_| write_failure(&path))
        }
    }

    fn ensure_metadata(&self, hash: &str, metadata: &JsonValue) -> YanshuResult<()> {
        let path = self.root().join("metadata").join(format!("{hash}.json"));
        if path.exists() {
            if self.version_metadata_unlocked(hash)? != *metadata {
                return Err(invalid_journal());
            }
            Ok(())
        } else {
            write_json_atomically(&path, metadata, MAXIMUM_METADATA_BYTES)
        }
    }

    fn write_event_log(&self, log: &EventLog) -> YanshuResult<()> {
        if u64::try_from(log.bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_EVENT_LOG_BYTES {
            return Err(invalid_journal());
        }
        let path = self.root().join("events.jsonl");
        atomic_replace(&path, &log.bytes).map_err(|_| write_failure(&path))
    }
}
