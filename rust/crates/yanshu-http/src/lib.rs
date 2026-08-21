#![forbid(unsafe_code)]

mod auth;
mod config;
mod dispatch;
mod loader;
mod observation;
mod request;
mod response;
mod router;
mod shadow;
mod transport;

pub use auth::BearerAuth;
pub use config::HttpConfig;
pub use loader::{ActiveVersionLoader, FixedProgramLoader, LoadedProgram, ProgramLoader};
pub use observation::{JsonlObservationSink, ObservationSink, RequestObservation};
pub use request::normalize_http_request;
pub use router::{
    build_active_router, build_active_router_with_auth, build_active_router_with_controls,
    build_active_router_with_runtime_controls, build_router, build_router_with_auth,
    build_router_with_controls, build_router_with_runtime_controls,
};
pub use shadow::ShadowControls;
pub use transport::serve_with_shutdown;

#[cfg(test)]
mod tests;
