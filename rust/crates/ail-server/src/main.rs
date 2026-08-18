#![forbid(unsafe_code)]

use std::{env, io::Write, process::ExitCode};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_http::{HttpConfig, build_active_router, serve_with_shutdown};
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
    let router = build_active_router(code_store, data_store, HttpConfig::default())?;
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
        println!(
            "{}",
            json!({
                "ok": true,
                "server": {
                    "address": address.to_string(),
                    "codeStore": code_store,
                    "dataStore": data_store,
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

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn rejects_invalid_arguments_before_starting_a_runtime() {
        let diagnostic = match run(vec!["only-one".to_owned()]) {
            Ok(()) => panic!("invalid server arguments must fail"),
            Err(diagnostic) => diagnostic,
        };
        assert_eq!(diagnostic.code, "CLI_USAGE");
    }
}
