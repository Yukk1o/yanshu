#![forbid(unsafe_code)]

use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    match yanshu_lsp::run_stdio(&mut reader, &mut writer) {
        Ok(()) => ExitCode::SUCCESS,
        Err(diagnostic) => {
            eprintln!("{}", diagnostic.public_json());
            ExitCode::FAILURE
        }
    }
}
