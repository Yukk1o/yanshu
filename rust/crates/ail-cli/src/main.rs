#![forbid(unsafe_code)]

use std::{env, fs, process::ExitCode};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::load_program_source;
use serde_json::{Value, json};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(document) => {
            println!("{document}");
            ExitCode::SUCCESS
        }
        Err(diagnostic) => {
            println!("{}", diagnostic.public_json());
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> AilResult<Value> {
    match arguments.as_slice() {
        [command, path] if matches!(command.as_str(), "check" | "inspect") => {
            let source = fs::read_to_string(path).map_err(|error| {
                Diagnostic::new(
                    "HOST_FILE_READ",
                    "host could not read the source file",
                    json!({ "path": path, "kind": error.kind().to_string() }),
                )
            })?;
            let program = load_program_source(&source)?;
            Ok(json!({ "ok": true, "program": program.inspect_json() }))
        }
        _ => Err(Diagnostic::new(
            "CLI_USAGE",
            "arguments do not match a supported Rust host command",
            json!({ "usage": ["check <program.ail>", "inspect <program.ail>"] }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use ail_diagnostic::{AilResult, Diagnostic};
    use serde_json::Value;

    use super::run;

    fn require_error(result: AilResult<Value>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    #[test]
    fn rejects_unknown_commands_without_panicking() {
        let result = run(vec!["unknown".to_owned()]);
        let diagnostic = require_error(result);
        assert_eq!(diagnostic.code, "CLI_USAGE");
    }
}
