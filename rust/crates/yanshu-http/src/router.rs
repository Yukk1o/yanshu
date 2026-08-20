#![forbid(unsafe_code)]

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use axum::Router;
use tokio::sync::Semaphore;
use yanshu_diagnostic::YanshuResult;
use yanshu_service::FileKvStore;

use crate::{
    auth::BearerAuth,
    config::{self, HttpConfig},
    dispatch::dispatch,
    loader::{ActiveVersionLoader, ProgramLoader},
    observation::ObservationSink,
    shadow::ShadowControls,
};

#[derive(Clone)]
pub(crate) struct HttpState {
    pub(crate) loader: Arc<dyn ProgramLoader>,
    pub(crate) store: Arc<Mutex<FileKvStore>>,
    pub(crate) permits: Arc<Semaphore>,
    pub(crate) config: HttpConfig,
    pub(crate) authentication: Option<Arc<BearerAuth>>,
    pub(crate) observations: Option<Arc<dyn ObservationSink>>,
    pub(crate) shadow: Option<ShadowControls>,
}

pub fn build_router(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
) -> YanshuResult<Router> {
    build_router_with_auth(loader, store, config, None)
}

pub fn build_router_with_auth(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
) -> YanshuResult<Router> {
    build_router_with_controls(loader, store, config, authentication, None)
}

pub fn build_router_with_controls(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
    observations: Option<Arc<dyn ObservationSink>>,
) -> YanshuResult<Router> {
    build_router_with_runtime_controls(loader, store, config, authentication, observations, None)
}

pub fn build_router_with_runtime_controls(
    loader: Arc<dyn ProgramLoader>,
    store: FileKvStore,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
    observations: Option<Arc<dyn ObservationSink>>,
    shadow: Option<ShadowControls>,
) -> YanshuResult<Router> {
    config::validate(&config)?;
    let state = HttpState {
        loader,
        store: Arc::new(Mutex::new(store)),
        permits: Arc::new(Semaphore::new(config.maximum_concurrency)),
        config,
        authentication: authentication.map(Arc::new),
        observations,
        shadow,
    };
    Ok(Router::new().fallback(dispatch).with_state(state))
}

pub fn build_active_router(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
) -> YanshuResult<Router> {
    build_active_router_with_auth(code_store, data_store, config, None)
}

pub fn build_active_router_with_auth(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
) -> YanshuResult<Router> {
    build_active_router_with_controls(code_store, data_store, config, authentication, None)
}

pub fn build_active_router_with_controls(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
    observations: Option<Arc<dyn ObservationSink>>,
) -> YanshuResult<Router> {
    build_active_router_with_runtime_controls(
        code_store,
        data_store,
        config,
        authentication,
        observations,
        None,
    )
}

pub fn build_active_router_with_runtime_controls(
    code_store: impl AsRef<Path>,
    data_store: impl AsRef<Path>,
    config: HttpConfig,
    authentication: Option<BearerAuth>,
    observations: Option<Arc<dyn ObservationSink>>,
    shadow: Option<ShadowControls>,
) -> YanshuResult<Router> {
    let loader: Arc<dyn ProgramLoader> = Arc::new(ActiveVersionLoader::new(code_store));
    let store = FileKvStore::open(data_store)?;
    build_router_with_runtime_controls(loader, store, config, authentication, observations, shadow)
}
