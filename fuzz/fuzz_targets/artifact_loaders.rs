#![forbid(unsafe_code)]
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use yanshu_compiler::{load_bytecode_envelope, load_wasm_bytecode};
use yanshu_syntax::{Program, load_program_source};

const PROGRAM_SOURCE: &str = r#"
(program
  (name fuzz-artifact)
  (version 4)
  (capabilities)
  (signature main (fn () integer))
  (def main (fn () 1))
  (export main))
"#;

fn fixture_program() -> &'static Program {
    static PROGRAM: OnceLock<Program> = OnceLock::new();
    PROGRAM.get_or_init(|| match load_program_source(PROGRAM_SOURCE) {
        Ok(program) => program,
        Err(_) => panic!("static fuzz fixture must remain valid"),
    })
}

fuzz_target!(|data: &[u8]| {
    let program = fixture_program();
    let _ = load_bytecode_envelope(program, data);
    let _ = load_wasm_bytecode(program, data);
});
