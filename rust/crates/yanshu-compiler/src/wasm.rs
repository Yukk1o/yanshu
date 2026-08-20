#![forbid(unsafe_code)]

use std::{fs, path::Path};

use serde_json::{Value, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::Program;

use crate::{BytecodeArtifact, artifact::canonical_json, compile_bytecode};

const WASM_MAGIC_AND_VERSION: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
const WASM_TARGET: &str = "yanshu-wasm-bytecode-v1";
const WASM_META_SECTION: &str = "yanshu.meta.v1";
const WASM_BYTECODE_SECTION: &str = "yanshu.bytecode.v1";
const YANSHU_WASM_ABI_FORMAT_VERSION: u8 = 1;
const MAXIMUM_WASM_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmArtifact {
    bytes: Vec<u8>,
    content_hash: String,
    bytecode_content_hash: String,
    static_instruction_weight: u64,
    guest_exports: Vec<String>,
}

impl WasmArtifact {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    #[must_use]
    pub fn bytecode_content_hash(&self) -> &str {
        &self.bytecode_content_hash
    }

    #[must_use]
    pub const fn static_instruction_weight(&self) -> u64 {
        self.static_instruction_weight
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "target": WASM_TARGET,
            "abiFormatVersion": YANSHU_WASM_ABI_FORMAT_VERSION,
            "contentHash": self.content_hash,
            "bytecodeContentHash": self.bytecode_content_hash,
            "byteLength": self.bytes.len(),
            "staticInstructionWeight": self.static_instruction_weight,
            "imports": ["yanshu_v1.execute"],
            "exports": ["yanshu_format_version", "yanshu_static_instruction_weight", "yanshu_run"],
            "abi": "yanshu_run(export_index: i32, arguments_handle: i32, fuel: i64) -> result_handle: i64",
            "guestExports": self.guest_exports,
            "execution": "trusted-yanshu-bytecode-vm",
        })
    }
}

pub fn compile_wasm(bytecode: &BytecodeArtifact) -> YanshuResult<WasmArtifact> {
    let static_weight = bytecode.static_instruction_weight();
    let bytecode_content_hash = bytecode.content_hash();
    let metadata = canonical_json(&json!({
        "target": WASM_TARGET,
        "abiFormatVersion": YANSHU_WASM_ABI_FORMAT_VERSION,
        "bytecodeContentHash": bytecode_content_hash,
        "programHash": bytecode.program_hash(),
        "capabilityClosure": bytecode.capability_closure(),
        "guestExports": bytecode.exports(),
        "staticInstructionWeight": static_weight,
        "fuelModel": "one unit per source-expression charge point plus metered value, primitive, schema, library, and capability costs",
    }));
    let encoded_bytecode = canonical_json(&bytecode.envelope_json());

    let mut bytes = WASM_MAGIC_AND_VERSION.to_vec();
    append_type_section(&mut bytes);
    append_import_section(&mut bytes);
    append_function_section(&mut bytes);
    append_export_section(&mut bytes);
    append_code_section(&mut bytes, static_weight)?;
    append_custom_section(&mut bytes, WASM_META_SECTION, &metadata)?;
    append_custom_section(&mut bytes, WASM_BYTECODE_SECTION, &encoded_bytecode)?;
    if bytes.len() > MAXIMUM_WASM_BYTES {
        return Err(Diagnostic::new(
            "WASM_ARTIFACT_LIMIT",
            "compiled WASM artifact exceeds its size limit",
            json!({ "maximum": MAXIMUM_WASM_BYTES, "actual": bytes.len() }),
        ));
    }
    let content_hash = crate::artifact::sha256(&bytes);
    Ok(WasmArtifact {
        bytes,
        content_hash,
        bytecode_content_hash,
        static_instruction_weight: static_weight,
        guest_exports: bytecode.exports().to_vec(),
    })
}

pub fn write_wasm_artifact(artifact: &WasmArtifact, path: impl AsRef<Path>) -> YanshuResult<()> {
    fs::write(path.as_ref(), artifact.bytes()).map_err(|error| {
        Diagnostic::new(
            "WASM_ARTIFACT_WRITE",
            "host could not write the WASM artifact",
            json!({ "kind": error.kind().to_string() }),
        )
    })
}

pub fn load_wasm_bytecode(program: &Program, source: &[u8]) -> YanshuResult<BytecodeArtifact> {
    if source.len() > MAXIMUM_WASM_BYTES {
        return Err(Diagnostic::new(
            "WASM_ARTIFACT_LIMIT",
            "WASM artifact exceeds its size limit",
            json!({ "maximum": MAXIMUM_WASM_BYTES, "actual": source.len() }),
        ));
    }
    let bytecode = compile_bytecode(program)?;
    let expected = compile_wasm(&bytecode)?;
    if source != expected.bytes() {
        return Err(Diagnostic::new(
            "WASM_ARTIFACT_MISMATCH",
            "WASM artifact is not the canonical compilation of the supplied program",
            json!({
                "expectedContentHash": expected.content_hash(),
                "programHash": bytecode.program_hash(),
            }),
        ));
    }
    Ok(bytecode)
}

fn append_type_section(module: &mut Vec<u8>) {
    let payload = [
        0x03, // three function types
        0x60, 0x00, 0x01, 0x7f, // () -> i32
        0x60, 0x00, 0x01, 0x7e, // () -> i64
        0x60, 0x03, 0x7f, 0x7f, 0x7e, 0x01, 0x7e, // (i32, i32, i64) -> i64
    ];
    append_section(module, 1, &payload);
}

fn append_import_section(module: &mut Vec<u8>) {
    let mut payload = Vec::new();
    encode_u64(1, &mut payload);
    append_name(&mut payload, "yanshu_v1");
    append_name(&mut payload, "execute");
    payload.extend([0x00, 0x02]);
    append_section(module, 2, &payload);
}

fn append_function_section(module: &mut Vec<u8>) {
    append_section(module, 3, &[0x03, 0x00, 0x01, 0x02]);
}

fn append_export_section(module: &mut Vec<u8>) {
    let mut payload = Vec::new();
    encode_u64(3, &mut payload);
    append_name(&mut payload, "yanshu_format_version");
    payload.extend([0x00, 0x01]);
    append_name(&mut payload, "yanshu_static_instruction_weight");
    payload.extend([0x00, 0x02]);
    append_name(&mut payload, "yanshu_run");
    payload.extend([0x00, 0x03]);
    append_section(module, 7, &payload);
}

fn append_code_section(module: &mut Vec<u8>, static_weight: u64) -> YanshuResult<()> {
    let mut payload = Vec::new();
    encode_u64(3, &mut payload);

    let version_body = [0x00, 0x41, YANSHU_WASM_ABI_FORMAT_VERSION, 0x0b];
    encode_u64(version_body.len() as u64, &mut payload);
    payload.extend(version_body);

    let signed_weight = i64::try_from(static_weight).map_err(|_| {
        Diagnostic::simple(
            "WASM_FUEL_LIMIT",
            "static semantic weight cannot be represented by the WASM metadata export",
        )
    })?;
    let mut fuel_body = vec![0x00, 0x42];
    encode_i64(signed_weight, &mut fuel_body);
    fuel_body.push(0x0b);
    encode_u64(fuel_body.len() as u64, &mut payload);
    payload.extend(fuel_body);

    let run_body = [
        0x00, // no locals
        0x20, 0x00, // local.get export_index
        0x20, 0x01, // local.get arguments_handle
        0x20, 0x02, // local.get fuel
        0x10, 0x00, // call imported yanshu_v1.execute
        0x0b, // end
    ];
    encode_u64(run_body.len() as u64, &mut payload);
    payload.extend(run_body);

    append_section(module, 10, &payload);
    Ok(())
}

fn append_custom_section(module: &mut Vec<u8>, name: &str, content: &[u8]) -> YanshuResult<()> {
    let mut payload = Vec::new();
    append_name(&mut payload, name);
    payload.extend_from_slice(content);
    if payload.len() > MAXIMUM_WASM_BYTES {
        return Err(Diagnostic::simple(
            "WASM_SECTION_LIMIT",
            "WASM custom section exceeds its size limit",
        ));
    }
    append_section(module, 0, &payload);
    Ok(())
}

fn append_section(module: &mut Vec<u8>, identifier: u8, payload: &[u8]) {
    module.push(identifier);
    encode_u64(payload.len() as u64, module);
    module.extend_from_slice(payload);
}

fn append_name(target: &mut Vec<u8>, name: &str) {
    encode_u64(name.len() as u64, target);
    target.extend_from_slice(name.as_bytes());
}

fn encode_u64(mut value: u64, target: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        target.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn encode_i64(mut value: i64, target: &mut Vec<u8>) {
    loop {
        let byte = (value as u8) & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        target.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use yanshu_diagnostic::{Diagnostic, YanshuResult};
    use yanshu_syntax::load_program_source;

    use super::{WASM_MAGIC_AND_VERSION, compile_wasm, load_wasm_bytecode};
    use crate::compile_bytecode;

    fn require<T>(result: YanshuResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn require_error<T>(result: YanshuResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    #[test]
    fn emits_deterministic_valid_wasm_envelope() {
        let program = require(load_program_source(
            r#"(program
                (name wasm-example)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value) (+ value 1)))
                (export run))"#,
        ));
        let bytecode = require(compile_bytecode(&program));
        let first = require(compile_wasm(&bytecode));
        let second = require(compile_wasm(&bytecode));
        assert_eq!(first, second);
        assert!(first.bytes().starts_with(&WASM_MAGIC_AND_VERSION));
        assert!(first.static_instruction_weight() > 0);
        let loaded = require(load_wasm_bytecode(&program, first.bytes()));
        assert_eq!(loaded.content_hash(), bytecode.content_hash());

        let mut tampered = first.bytes().to_vec();
        let last = tampered
            .last_mut()
            .unwrap_or_else(|| panic!("WASM fixture cannot be empty"));
        *last ^= 1;
        let diagnostic = require_error(load_wasm_bytecode(&program, &tampered));
        assert_eq!(diagnostic.code, "WASM_ARTIFACT_MISMATCH");
    }
}
