#![forbid(unsafe_code)]

use std::{env, io::Write, net::SocketAddr, process::ExitCode, sync::Arc};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_http::{
    BearerAuth, HttpConfig, JsonlObservationSink, ObservationSink,
    build_active_router_with_controls, serve_with_shutdown,
};
use ail_ops::acquire_service_lease;
use serde_json::json;
use tokio::{net::TcpListener, runtime, signal};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(diagnostic) => {
            println!("{}", diagnostic.public_json());
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> AilResult<()> {
    let [code_store, bind_address, data_store] = arguments.as_slice() else {
        return Err(Diagnostic::new(
            "CLI_USAGE",
            "arguments do not match the Rust HTTP server command",
            json!({ "usage": "ail-server <code-store> <bind-address> <data-store.json>" }),
        ));
    };
    let _service_lease = acquire_service_lease(data_store)?;
    let authentication = configured_authentication()?;
    let authentication_required = authentication.is_some();
    let observation_path = format!("{data_store}.observations.jsonl");
    let observations: Arc<dyn ObservationSink> =
        Arc::new(JsonlObservationSink::open(&observation_path)?);
    let router = build_active_router_with_controls(
        code_store,
        data_store,
        HttpConfig::default(),
        authentication,
        Some(observations),
    )?;
    let runtime = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| {
            Diagnostic::simple(
                "HTTP_RUNTIME_FAILURE",
                "HTTP runtime could not be initialized",
            )
        })?;
    runtime.block_on(async {
        let listener = TcpListener::bind(bind_address).await.map_err(|_| {
            Diagnostic::new(
                "HTTP_BIND_FAILURE",
                "HTTP listener could not bind the configured address",
                json!({ "address": bind_address }),
            )
        })?;
        let address = listener.local_addr().map_err(|_| {
            Diagnostic::simple("HTTP_BIND_FAILURE", "HTTP listener address is unavailable")
        })?;
        require_loopback(address)?;
        println!(
            "{}",
            json!({
                "ok": true,
                "server": {
                    "address": address.to_string(),
                    "codeStore": code_store,
                    "dataStore": data_store,
                    "observationStore": observation_path,
                    "authenticationRequired": authentication_required,
                }
            })
        );
        std::io::stdout().flush().map_err(|_| {
            Diagnostic::simple(
                "HTTP_STDOUT_FAILURE",
                "server startup output could not be flushed",
            )
        })?;
        serve_with_shutdown(listener, router, async {
            let _ignored = signal::ctrl_c().await;
        })
        .await
        .map_err(|_| Diagnostic::simple("HTTP_SERVER_FAILURE", "HTTP server stopped unexpectedly"))
    })
}

fn configured_authentication() -> AilResult<Option<BearerAuth>> {
    match env::var("AI_EVOLVE_HTTP_BEARER_TOKEN") {
        Ok(token) => BearerAuth::new(token).map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(Diagnostic::simple(
            "HTTP_INVALID_AUTH_CONFIG",
            "HTTP Bearer token must be valid Unicode",
        )),
    }
}

fn require_loopback(address: SocketAddr) -> AilResult<()> {
    if address.ip().is_loopback() {
        Ok(())
    } else {
        Err(Diagnostic::new(
            "HTTP_NON_LOOPBACK_FORBIDDEN",
            "Rust HTTP server must bind a loopback address behind a trusted reverse proxy",
            json!({ "address": address.to_string() }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{require_loopback, run};

    #[test]
    fn rejects_invalid_arguments_before_starting_a_runtime() {
        let diagnostic = match run(vec!["only-one".to_owned()]) {
            Ok(()) => panic!("invalid server arguments must fail"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(diagnostic.code, "CLI_USAGE");
    }

    #[test]
    fn permits_only_loopback_listener_addresses() {
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        assert!(require_loopback(loopback).is_ok());
        let wildcard = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080);
        let diagnostic = match require_loopback(wildcard) {
            Ok(()) => panic!("wildcard listener must fail"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(diagnostic.code, "HTTP_NON_LOOPBACK_FORBIDDEN");
    }
}
