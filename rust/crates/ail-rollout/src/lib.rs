#![forbid(unsafe_code)]

mod comparison;
mod observation;
mod policy;
mod runtime;

pub use comparison::{ExecutionSummary, ShadowComparison};
pub use observation::{
    JsonlShadowObservationSink, ShadowObservation, ShadowObservationSink, ShadowOutcome,
};
pub use policy::ShadowPolicy;
pub use runtime::{PreparedShadow, ShadowPreparation, ShadowRuntime};
