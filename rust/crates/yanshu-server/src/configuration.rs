use std::{env, sync::Arc};

use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_http::ShadowControls;
use yanshu_rollout::{
    JsonlShadowObservationSink, ShadowObservationSink, ShadowPolicy, ShadowRuntime,
};

pub struct ConfiguredShadow {
    pub controls: ShadowControls,
    pub candidate_version: String,
    pub observation_path: String,
}

pub fn configured_shadow(
    code_store: &str,
    data_store: &str,
) -> YanshuResult<Option<ConfiguredShadow>> {
    let version = optional_variable("YANSHU_SHADOW_VERSION")?;
    let percent = optional_variable("YANSHU_SHADOW_PERCENT")?;
    let concurrency = optional_variable("YANSHU_SHADOW_MAX_CONCURRENCY")?;
    let Some(version) = version else {
        if percent.is_some() || concurrency.is_some() {
            return Err(invalid_shadow_config(
                "shadow percentage or concurrency requires YANSHU_SHADOW_VERSION",
            ));
        }
        return Ok(None);
    };
    let percent = percent.ok_or_else(|| {
        invalid_shadow_config(
            "YANSHU_SHADOW_PERCENT is required when a shadow version is configured",
        )
    })?;
    let percent = percent.parse::<u8>().map_err(|_| {
        invalid_shadow_config("YANSHU_SHADOW_PERCENT must be an integer from 1 to 100")
    })?;
    if percent == 0 || percent > 100 {
        return Err(invalid_shadow_config(
            "YANSHU_SHADOW_PERCENT must be an integer from 1 to 100",
        ));
    }
    let maximum_concurrency = concurrency.map_or(Ok(4_usize), |value| {
        value.parse::<usize>().map_err(|_| {
            invalid_shadow_config("YANSHU_SHADOW_MAX_CONCURRENCY must be a positive integer")
        })
    })?;
    if maximum_concurrency == 0 {
        return Err(invalid_shadow_config(
            "YANSHU_SHADOW_MAX_CONCURRENCY must be a positive integer",
        ));
    }

    let observation_path = format!("{data_store}.shadow.jsonl");
    let observations: Arc<dyn ShadowObservationSink> =
        Arc::new(JsonlShadowObservationSink::open(&observation_path)?);
    let policy = ShadowPolicy::new(&version, percent)?;
    let runtime = Arc::new(ShadowRuntime::new(code_store, policy, observations));
    let controls = ShadowControls::new(runtime, maximum_concurrency)?;
    Ok(Some(ConfiguredShadow {
        controls,
        candidate_version: version,
        observation_path,
    }))
}

fn optional_variable(name: &'static str) -> YanshuResult<Option<String>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() && value.trim() == value => Ok(Some(value)),
        Ok(_) => Err(invalid_shadow_config(
            "shadow environment variables must be non-empty and have no surrounding whitespace",
        )),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_shadow_config(
            "shadow environment variables must be valid Unicode",
        )),
    }
}

fn invalid_shadow_config(message: &'static str) -> Diagnostic {
    Diagnostic::simple("SHADOW_INVALID_CONFIG", message)
}
