#![forbid(unsafe_code)]

mod artifact;
mod bytecode;
mod compile;
mod verify;
mod wasm;

pub use artifact::{
    BytecodeArtifact, compile_bytecode, load_bytecode_envelope, write_bytecode_envelope,
};
pub use bytecode::{CodeBlock, DefinitionCode, Instruction, LocatedInstruction};
pub use verify::verify_bytecode;
pub use wasm::{WasmArtifact, compile_wasm, load_wasm_bytecode, write_wasm_artifact};
