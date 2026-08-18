use std::sync::Arc;

use ail_diagnostic::{AilResult, Diagnostic};
use ail_rollout::{
    ShadowComparison, ShadowObservation, ShadowOutcome, ShadowPreparation, ShadowRuntime,
};
use ail_service::{DispatchResult, MemoryKvStore, ServiceRequest, handle_service_request_with_id};
use num_bigint::BigInt;
use serde_json::json;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task,
};

#[derive(Clone)]
pub struct ShadowControls {
    runtime: Arc<ShadowRuntime>,
    permits: Arc<Semaphore>,
}

impl ShadowControls {
    pub fn new(runtime: Arc<ShadowRuntime>, maximum_concurrency: usize) -> AilResult<Self> {
        if maximum_concurrency == 0 {
            return Err(Diagnostic::simple(
                "SHADOW_INVALID_CONCURRENCY",
                "shadow concurrency limit must be positive",
            ));
        }
        Ok(Self {
            runtime,
            permits: Arc::new(Semaphore::new(maximum_concurrency)),
        })
    }

    pub(crate) fn admit(&self, request_id: &str) -> Admission {
        if !self.runtime.selects(request_id) {
            return Admission::NotSelected;
        }
        match Arc::clone(&self.permits).try_acquire_owned() {
            Ok(permit) => Admission::Admitted {
                runtime: Arc::clone(&self.runtime),
                permit,
            },
            Err(_) => Admission::CapacitySkipped {
                runtime: Arc::clone(&self.runtime),
            },
        }
    }
}

pub(crate) enum Admission {
    NotSelected,
    Admitted {
        runtime: Arc<ShadowRuntime>,
        permit: OwnedSemaphorePermit,
    },
    CapacitySkipped {
        runtime: Arc<ShadowRuntime>,
    },
}

impl Admission {
    #[must_use]
    pub(crate) fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted { .. })
    }

    pub(crate) fn record_capacity_skip(
        &self,
        timestamp_ms: u64,
        request_id: &str,
        active_version: Option<String>,
    ) {
        let Self::CapacitySkipped { runtime } = self else {
            return;
        };
        let observation = ShadowObservation {
            timestamp_ms,
            request_id: request_id.to_owned(),
            active_version,
            candidate_version: runtime.candidate_version().to_owned(),
            outcome: ShadowOutcome::CapacitySkipped,
        };
        if let Err(diagnostic) = runtime.record(&observation) {
            report_failure(request_id, &diagnostic);
        }
    }

    pub(crate) fn into_job(
        self,
        store: Option<MemoryKvStore>,
        context: JobContext,
    ) -> Option<Box<ShadowJob>> {
        let Self::Admitted { runtime, permit } = self else {
            return None;
        };
        Some(Box::new(ShadowJob {
            runtime,
            _permit: permit,
            store: store?,
            request: context.request,
            request_id: context.request_id,
            clock_milliseconds: context.clock_milliseconds,
            active_version: context.active_version,
            active_result: context.active_result,
            timestamp_ms: context.timestamp_ms,
        }))
    }
}

pub(crate) struct JobContext {
    pub request: ServiceRequest,
    pub request_id: String,
    pub clock_milliseconds: BigInt,
    pub active_version: Option<String>,
    pub active_result: DispatchResult,
    pub timestamp_ms: u64,
}

pub(crate) struct ShadowJob {
    runtime: Arc<ShadowRuntime>,
    _permit: OwnedSemaphorePermit,
    store: MemoryKvStore,
    request: ServiceRequest,
    request_id: String,
    clock_milliseconds: BigInt,
    active_version: Option<String>,
    active_result: DispatchResult,
    timestamp_ms: u64,
}

pub(crate) fn launch(job: Box<ShadowJob>) {
    let _task = task::spawn_blocking(move || execute(*job));
}

fn execute(mut job: ShadowJob) {
    let outcome = match job.runtime.prepare_selected() {
        ShadowPreparation::Ready(candidate) => {
            let result = handle_service_request_with_id(
                &candidate.program,
                &job.request,
                &mut job.store,
                &job.clock_milliseconds,
                &job.request_id,
            );
            ShadowOutcome::Compared(Box::new(ShadowComparison::compare(
                &job.active_result,
                &result,
            )))
        }
        ShadowPreparation::Unavailable { error_code } => {
            ShadowOutcome::CandidateUnavailable { error_code }
        }
        ShadowPreparation::NotSelected => return,
    };
    let observation = ShadowObservation {
        timestamp_ms: job.timestamp_ms,
        request_id: job.request_id.clone(),
        active_version: job.active_version,
        candidate_version: job.runtime.candidate_version().to_owned(),
        outcome,
    };
    if let Err(diagnostic) = job.runtime.record(&observation) {
        report_failure(&job.request_id, &diagnostic);
    }
}

fn report_failure(request_id: &str, diagnostic: &Diagnostic) {
    eprintln!(
        "{}",
        json!({
            "ok": false,
            "event": "shadow-observation-write-failed",
            "requestId": request_id,
            "errorCode": diagnostic.code,
        })
    );
}
