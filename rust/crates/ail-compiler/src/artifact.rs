#![forbid(unsafe_code)]

use std::{fs, path::Path};

use ail_analysis::analyze_program;
use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::Program;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{CodeBlock, DefinitionCode, compile::lower_program, verify_bytecode};

const BYTECODE_FORMAT_VERSION: u64 = 1;
const BYTECODE_TARGET: &str = "ail-bytecode-v1";
const MAXIMUM_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeArtifact {
    program: Program,
    program_hash: String,
    capability_closure: Vec<String>,
    definitions: Vec<DefinitionCode>,
    blocks: Vec<CodeBlock>,
}

impl BytecodeArtifact {
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    #[must_use]
    pub fn program_hash(&self) -> &str {
        &self.program_hash
    }

    #[must_use]
    pub fn capability_closure(&self) -> &[String] {
        &self.capability_closure
    }

    #[must_use]
    pub fn definitions(&self) -> &[DefinitionCode] {
        &self.definitions
    }

    #[must_use]
    pub fn blocks(&self) -> &[CodeBlock] {
        &self.blocks
    }

    #[must_use]
    pub fn exports(&self) -> &[String] {
        &self.program.exports
    }

    #[must_use]
    pub fn static_instruction_weight(&self) -> u64 {
        self.blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .fold(0_u64, |fuel, located| {
                fuel.saturating_add(located.instruction.fuel_cost())
            })
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "formatVersion": BYTECODE_FORMAT_VERSION,
            "target": BYTECODE_TARGET,
            "programHash": self.program_hash,
            "programName": self.program.name,
            "languageVersion": self.program.version.to_string(),
            "capabilityClosure": self.capability_closure,
            "exports": self.program.exports,
            "definitions": self.definitions.iter().map(DefinitionCode::to_json).collect::<Vec<_>>(),
            "blocks": self.blocks.iter().map(CodeBlock::to_json).collect::<Vec<_>>(),
            "staticInstructionWeight": self.static_instruction_weight(),
        })
    }

    #[must_use]
    pub fn content_hash(&self) -> String {
        sha256(&canonical_json(&self.to_json()))
    }

    #[must_use]
    pub fn envelope_json(&self) -> Value {
        json!({
            "formatVersion": BYTECODE_FORMAT_VERSION,
            "contentHash": self.content_hash(),
            "artifact": self.to_json(),
        })
    }
}

pub fn compile_bytecode(program: &Program) -> AilResult<BytecodeArtifact> {
    if !program.imports.is_empty() {
        return Err(Diagnostic::new(
            "COMPILER_UNLINKED_IMPORTS",
            "bytecode compilation requires a linked program",
            json!({ "imports": program.imports }),
        ));
    }
    if program.version.to_string() != "4" {
        return Err(Diagnostic::new(
            "COMPILER_LANGUAGE_VERSION",
            "v0.10 bytecode compilation requires language version 4",
            json!({ "actual": program.version.to_string(), "required": "4" }),
        ));
    }
    let analysis = analyze_program(program)?;
    let compilation = lower_program(program)?;
    let artifact = BytecodeArtifact {
        program: program.clone(),
        program_hash: semantic_program_hash(program),
        capability_closure: analysis.capability_closure,
        definitions: compilation.definitions,
        blocks: compilation.blocks,
    };
    verify_bytecode(&artifact)?;
    Ok(artifact)
}

pub fn write_bytecode_envelope(
    artifact: &BytecodeArtifact,
    path: impl AsRef<Path>,
) -> AilResult<()> {
    let bytes = canonical_json(&artifact.envelope_json());
    if bytes.len() > MAXIMUM_ARTIFACT_BYTES {
        return Err(Diagnostic::new(
            "BYTECODE_ARTIFACT_LIMIT",
            "bytecode artifact exceeds its encoded size limit",
            json!({ "maximum": MAXIMUM_ARTIFACT_BYTES, "actual": bytes.len() }),
        ));
    }
    fs::write(path.as_ref(), bytes).map_err(|error| {
        Diagnostic::new(
            "COMPILER_ARTIFACT_WRITE",
            "host could not write the bytecode artifact",
            json!({ "kind": error.kind().to_string() }),
        )
    })
}

pub fn load_bytecode_envelope(program: &Program, source: &[u8]) -> AilResult<BytecodeArtifact> {
    if source.len() > MAXIMUM_ARTIFACT_BYTES {
        return Err(Diagnostic::new(
            "BYTECODE_ARTIFACT_LIMIT",
            "bytecode artifact exceeds its encoded size limit",
            json!({ "maximum": MAXIMUM_ARTIFACT_BYTES, "actual": source.len() }),
        ));
    }
    let document: Value = serde_json::from_slice(source).map_err(|error| {
        Diagnostic::new(
            "BYTECODE_ARTIFACT_JSON",
            "bytecode artifact is not valid JSON",
            json!({ "line": error.line(), "column": error.column() }),
        )
    })?;
    let expected = compile_bytecode(program)?;
    if document != expected.envelope_json() {
        return Err(Diagnostic::new(
            "BYTECODE_ARTIFACT_MISMATCH",
            "bytecode artifact is not the canonical compilation of the supplied program",
            json!({
                "expectedContentHash": expected.content_hash(),
                "programHash": expected.program_hash(),
            }),
        ));
    }
    Ok(expected)
}

pub(crate) fn canonical_json(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn semantic_program_hash(program: &Program) -> String {
    sha256(&canonical_json(&program.inspect_json()))
}

#[cfg(test)]
mod tests {
    use ail_diagnostic::{AilResult, Diagnostic};
    use ail_syntax::load_program_source;

    use super::{canonical_json, compile_bytecode, load_bytecode_envelope};
    use crate::{Instruction, verify_bytecode};

    fn require<T>(result: AilResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn require_error<T>(result: AilResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    fn program() -> ail_syntax::Program {
        require(load_program_source(
            r#"(program
                (name artifact-test)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value)
                  (cond
                    ((< value 0) (- value))
                    ((= value 0) 1)
                    (else (+ value 1)))))
                (export run))"#,
        ))
    }

    #[test]
    fn envelope_is_canonical_and_bound_to_program_semantics() {
        let program = program();
        let artifact = require(compile_bytecode(&program));
        let encoded = canonical_json(&artifact.envelope_json());
        let loaded = require(load_bytecode_envelope(&program, &encoded));
        assert_eq!(loaded.content_hash(), artifact.content_hash());

        let mut tampered: serde_json::Value = serde_json::from_slice(&encoded)
            .unwrap_or_else(|error| panic!("artifact JSON failed: {error}"));
        tampered["contentHash"] = serde_json::Value::String("0".repeat(64));
        let diagnostic =
            require_error(load_bytecode_envelope(&program, &canonical_json(&tampered)));
        assert_eq!(diagnostic.code, "BYTECODE_ARTIFACT_MISMATCH");
    }

    #[test]
    fn verifier_rejects_invalid_control_flow_without_panicking() {
        let mut artifact = require(compile_bytecode(&program()));
        artifact.blocks[0].instructions[0].instruction = Instruction::Jump(usize::MAX);
        let diagnostic = require_error(verify_bytecode(&artifact));
        assert_eq!(diagnostic.code, "BYTECODE_INVALID_JUMP");
    }
}
