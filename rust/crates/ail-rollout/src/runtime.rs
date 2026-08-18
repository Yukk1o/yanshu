use std::{path::Path, sync::Arc};

use ail_store::VersionStore;
use ail_syntax::{Program, load_program_source};

use crate::{ShadowObservation, ShadowObservationSink, ShadowOutcome, ShadowPolicy};

#[derive(Debug, Clone)]
pub struct PreparedShadow {
    pub version: String,
    pub program: Program,
}

#[derive(Debug, Clone)]
pub enum ShadowPreparation {
    NotSelected,
    Ready(Box<PreparedShadow>),
    Unavailable { error_code: String },
}

pub struct ShadowRuntime {
    store: VersionStore,
    policy: ShadowPolicy,
    observations: Arc<dyn ShadowObservationSink>,
}

impl ShadowRuntime {
    pub fn new(
        code_store: impl AsRef<Path>,
        policy: ShadowPolicy,
        observations: Arc<dyn ShadowObservationSink>,
    ) -> Self {
        Self {
            store: VersionStore::new(code_store),
            policy,
            observations,
        }
    }

    #[must_use]
    pub fn candidate_version(&self) -> &str {
        self.policy.candidate_version()
    }

    #[must_use]
    pub fn selects(&self, request_id: &str) -> bool {
        self.policy.selects(request_id)
    }

    #[must_use]
    pub fn prepare(&self, request_id: &str) -> ShadowPreparation {
        if !self.selects(request_id) {
            return ShadowPreparation::NotSelected;
        }
        self.prepare_selected()
    }

    #[must_use]
    pub fn prepare_selected(&self) -> ShadowPreparation {
        match self.load_candidate() {
            Ok(program) => ShadowPreparation::Ready(Box::new(PreparedShadow {
                version: self.policy.candidate_version().to_owned(),
                program,
            })),
            Err(diagnostic) => ShadowPreparation::Unavailable {
                error_code: diagnostic.code.to_owned(),
            },
        }
    }

    pub fn record(&self, observation: &ShadowObservation) -> ail_diagnostic::AilResult<()> {
        self.observations.record(observation)
    }

    pub fn record_unavailable(
        &self,
        timestamp_ms: u64,
        request_id: &str,
        active_version: Option<String>,
        error_code: String,
    ) -> ail_diagnostic::AilResult<()> {
        self.record(&ShadowObservation {
            timestamp_ms,
            request_id: request_id.to_owned(),
            active_version,
            candidate_version: self.candidate_version().to_owned(),
            outcome: ShadowOutcome::CandidateUnavailable { error_code },
        })
    }

    fn load_candidate(&self) -> ail_diagnostic::AilResult<Program> {
        self.store
            .version_metadata(self.policy.candidate_version())?;
        let source = self.store.version_source(self.policy.candidate_version())?;
        load_program_source(&source)
    }
}
