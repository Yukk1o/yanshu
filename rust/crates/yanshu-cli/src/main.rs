#![forbid(unsafe_code)]

use std::{
    env, fs,
    io::Read,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use yanshu_analysis::{analyze_program, render_rust_review};
use yanshu_bundle::{load_bundle, seal_bundle_directory};
use yanshu_compiler::{
    BytecodeArtifact, compile_bytecode, compile_wasm, load_bytecode_envelope, load_wasm_bytecode,
    write_bytecode_envelope, write_wasm_artifact,
};
use yanshu_conformance::run_manifest;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_format::{FORMATTER_VERSION, FormatOptions, format_source};
use yanshu_ops::{create_backup, restore_backup, verify_backup};
use yanshu_package::{load_locked_package, lock_workspace, pack_workspace, verify_package};
use yanshu_provider::{EvolutionProvider, EvolutionRequest, configured_evolution_provider};
use yanshu_runtime::{
    CapabilityHost, ExecutionOptions, ExecutionReport, Value as GuestValue,
    execute_compiled_export_with_host_report, execute_export_with_host, json_to_value,
};
use yanshu_service::run_service_suite;
use yanshu_store::{CandidateRegistration, VersionStore, run_version_scenario, source_hash};
use yanshu_syntax::{Program, load_program_source};

#[derive(Default)]
struct CliCapabilityHost {
    log_events: u64,
}

impl CapabilityHost for CliCapabilityHost {
    fn supports(&self, capability: &str) -> bool {
        capability == "log"
    }

    fn invoke(&mut self, operation: &str, _arguments: &[GuestValue]) -> YanshuResult<GuestValue> {
        if operation != "log" {
            return Err(Diagnostic::new(
                "RUNTIME_CAPABILITY_UNAVAILABLE",
                "CLI host does not provide the requested capability",
                json!({ "capability": operation }),
            ));
        }
        self.log_events = self.log_events.saturating_add(1);
        Ok(GuestValue::Nil)
    }
}

fn execute_cli_program(
    program: &Program,
    export: &str,
    arguments: Vec<GuestValue>,
) -> YanshuResult<(GuestValue, u64)> {
    let mut host = CliCapabilityHost::default();
    let value = execute_export_with_host(
        program,
        export,
        arguments,
        ExecutionOptions::default(),
        &mut host,
    )?;
    Ok((value, host.log_events))
}

fn execute_cli_compiled(
    artifact: &BytecodeArtifact,
    export: &str,
    arguments: Vec<GuestValue>,
) -> YanshuResult<(ExecutionReport, u64)> {
    let mut host = CliCapabilityHost::default();
    let report = execute_compiled_export_with_host_report(
        artifact,
        export,
        arguments,
        ExecutionOptions::default(),
        &mut host,
    )?;
    Ok((report, host.log_events))
}

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1).collect::<Vec<_>>();
    let text_mode = match arguments.as_slice() {
        [command, _, option]
            if matches!(command.as_str(), "review" | "review-bundle") && option == "--text" =>
        {
            true
        }
        [command, _, _, option] if command == "package-review" && option == "--text" => true,
        _ => false,
    };
    if text_mode {
        arguments.pop();
    }
    match run(arguments) {
        Ok(document) => {
            if text_mode {
                if let Some(text) = document
                    .get("review")
                    .and_then(|review| review.get("text"))
                    .and_then(Value::as_str)
                {
                    print!("{text}");
                } else {
                    println!("{document}");
                    return ExitCode::FAILURE;
                }
            } else {
                println!("{document}");
            }
            if document.get("ok").and_then(Value::as_bool) == Some(false) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(diagnostic) => {
            println!("{}", diagnostic.public_json());
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> YanshuResult<Value> {
    if arguments.iter().any(|argument| argument == "--text") {
        return Err(Diagnostic::new(
            "CLI_INVALID_OPTION",
            "--text is only valid after every required review path",
            json!({ "option": "--text" }),
        ));
    }
    match arguments.as_slice() {
        [command, program_path, output_path] if command == "compile-bytecode" => {
            let program = read_program_file(program_path)?;
            let artifact = compile_bytecode(&program)?;
            write_bytecode_envelope(&artifact, output_path)?;
            Ok(json!({
                "ok": true,
                "target": "yanshu-bytecode-v1",
                "output": output_path,
                "contentHash": artifact.content_hash(),
                "programHash": artifact.program_hash(),
                "staticInstructionWeight": artifact.static_instruction_weight(),
                "capabilityClosure": artifact.capability_closure(),
            }))
        }
        [command, program_path, artifact_path] if command == "inspect-bytecode" => {
            let program = read_program_file(program_path)?;
            let source = read_artifact_file(artifact_path, "bytecode")?;
            let artifact = load_bytecode_envelope(&program, &source)?;
            Ok(json!({
                "ok": true,
                "contentHash": artifact.content_hash(),
                "artifact": artifact.to_json(),
            }))
        }
        [command, program_path, artifact_path, export, arguments_path]
            if command == "run-bytecode" =>
        {
            let program = read_program_file(program_path)?;
            let source = read_artifact_file(artifact_path, "bytecode")?;
            let artifact = load_bytecode_envelope(&program, &source)?;
            let values = read_arguments_file(arguments_path, "bytecode")?;
            let (report, log_events) = execute_cli_compiled(&artifact, export, values)?;
            Ok(json!({
                "ok": true,
                "contentHash": artifact.content_hash(),
                "execution": report.cost_json(),
                "logEvents": log_events,
                "result": report.value.to_json()?,
            }))
        }
        [command, program_path, output_path] if command == "compile-wasm" => {
            let program = read_program_file(program_path)?;
            let bytecode = compile_bytecode(&program)?;
            let wasm = compile_wasm(&bytecode)?;
            write_wasm_artifact(&wasm, output_path)?;
            Ok(json!({ "ok": true, "output": output_path, "artifact": wasm.to_json() }))
        }
        [command, program_path, artifact_path] if command == "inspect-wasm" => {
            let program = read_program_file(program_path)?;
            let source = read_artifact_file(artifact_path, "WASM")?;
            let bytecode = load_wasm_bytecode(&program, &source)?;
            let wasm = compile_wasm(&bytecode)?;
            Ok(json!({ "ok": true, "artifact": wasm.to_json() }))
        }
        [command, program_path, artifact_path, export, arguments_path] if command == "run-wasm" => {
            let program = read_program_file(program_path)?;
            let source = read_artifact_file(artifact_path, "WASM")?;
            let artifact = load_wasm_bytecode(&program, &source)?;
            let values = read_arguments_file(arguments_path, "WASM")?;
            let (report, log_events) = execute_cli_compiled(&artifact, export, values)?;
            Ok(json!({
                "ok": true,
                "wasmContentHash": compile_wasm(&artifact)?.content_hash(),
                "bytecodeContentHash": artifact.content_hash(),
                "execution": report.cost_json(),
                "logEvents": log_events,
                "result": report.value.to_json()?,
            }))
        }
        [command, store, lock_path, bytecode_path, wasm_path] if command == "package-compile" => {
            let package = load_locked_package(store, lock_path)?;
            let bytecode = compile_bytecode(&package.program)?;
            let wasm = compile_wasm(&bytecode)?;
            write_bytecode_envelope(&bytecode, bytecode_path)?;
            write_wasm_artifact(&wasm, wasm_path)?;
            Ok(json!({
                "ok": true,
                "lockHash": package.lock_hash,
                "bytecode": {
                    "output": bytecode_path,
                    "contentHash": bytecode.content_hash(),
                    "staticInstructionWeight": bytecode.static_instruction_weight(),
                },
                "wasm": {
                    "output": wasm_path,
                    "artifact": wasm.to_json(),
                },
            }))
        }
        [command, store, lock_path, wasm_path, export, arguments_path]
            if command == "package-run-compiled" =>
        {
            let package = load_locked_package(store, lock_path)?;
            let source = read_artifact_file(wasm_path, "WASM")?;
            let artifact = load_wasm_bytecode(&package.program, &source)?;
            let values = read_arguments_file(arguments_path, "compiled package")?;
            let (report, log_events) = execute_cli_compiled(&artifact, export, values)?;
            Ok(json!({
                "ok": true,
                "lockHash": package.lock_hash,
                "wasmContentHash": compile_wasm(&artifact)?.content_hash(),
                "execution": report.cost_json(),
                "logEvents": log_events,
                "result": report.value.to_json()?,
            }))
        }
        [command, root, bytecode_path, wasm_path] if command == "compile-bundle" => {
            let bundle = load_bundle(root)?;
            let bytecode = compile_bytecode(&bundle.program)?;
            let wasm = compile_wasm(&bytecode)?;
            write_bytecode_envelope(&bytecode, bytecode_path)?;
            write_wasm_artifact(&wasm, wasm_path)?;
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "bytecodeContentHash": bytecode.content_hash(),
                "wasm": wasm.to_json(),
            }))
        }
        [command, root, wasm_path, export, arguments_path] if command == "run-bundle-compiled" => {
            let bundle = load_bundle(root)?;
            let source = read_artifact_file(wasm_path, "WASM")?;
            let artifact = load_wasm_bytecode(&bundle.program, &source)?;
            let values = read_arguments_file(arguments_path, "compiled bundle")?;
            let (report, log_events) = execute_cli_compiled(&artifact, export, values)?;
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "execution": report.cost_json(),
                "logEvents": log_events,
                "result": report.value.to_json()?,
            }))
        }
        [command, workspace, store] if command == "package-pack" => {
            let root_package = pack_workspace(workspace, store)?;
            let manifest = verify_package(store, &root_package)?;
            Ok(json!({
                "ok": true,
                "rootPackage": root_package,
                "manifest": manifest.to_json(),
            }))
        }
        [command, workspace, store, lock_path] if command == "package-lock" => {
            let lock = lock_workspace(workspace, store, lock_path)?;
            Ok(json!({
                "ok": true,
                "lockHash": lock.content_hash(),
                "lock": lock.to_json(),
            }))
        }
        [command, store, content_hash] if command == "package-verify" => {
            let manifest = verify_package(store, content_hash)?;
            Ok(json!({
                "ok": true,
                "contentHash": content_hash,
                "manifest": manifest.to_json(),
            }))
        }
        [command, store, lock_path] if command == "package-inspect" => {
            let package = load_locked_package(store, lock_path)?;
            let mut document = json!({
                "ok": true,
                "lockHash": package.lock_hash,
                "lock": package.lock.to_json(),
                "program": package.program.inspect_json(),
            });
            if package.program.version.to_string() == "4" {
                let analysis = analyze_program(&package.program)?;
                document["analysis"] = analysis.to_json();
                document["review"] = render_rust_review(&package.program, &analysis).to_json();
            }
            Ok(document)
        }
        [command, store, lock_path] if command == "package-review" => {
            let package = load_locked_package(store, lock_path)?;
            let analysis = analyze_program(&package.program)?;
            Ok(json!({
                "ok": true,
                "lockHash": package.lock_hash,
                "analysis": analysis.to_json(),
                "review": render_rust_review(&package.program, &analysis).to_json(),
            }))
        }
        [command, store, lock_path, export, arguments_path] if command == "package-run" => {
            let values = read_arguments_file(arguments_path, "package")?;
            let package = load_locked_package(store, lock_path)?;
            let (result, log_events) = execute_cli_program(&package.program, export, values)?;
            Ok(json!({
                "ok": true,
                "lockHash": package.lock_hash,
                "logEvents": log_events,
                "result": result.to_json()?,
            }))
        }
        [command, root, entry, module_paths @ ..]
            if command == "seal-bundle" && !module_paths.is_empty() =>
        {
            let manifest = seal_bundle_directory(root, entry, module_paths)?;
            Ok(json!({
                "ok": true,
                "bundleHash": manifest.content_hash(),
                "manifest": manifest.to_json(),
            }))
        }
        [command, root] if command == "inspect-bundle" => {
            let bundle = load_bundle(root)?;
            let mut document = json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "manifest": bundle.manifest.to_json(),
                "program": bundle.program.inspect_json(),
            });
            if bundle.program.version.to_string() == "4" {
                let analysis = analyze_program(&bundle.program)?;
                document["analysis"] = analysis.to_json();
                document["review"] = render_rust_review(&bundle.program, &analysis).to_json();
            }
            Ok(document)
        }
        [command, root] if command == "review-bundle" => {
            let bundle = load_bundle(root)?;
            let analysis = analyze_program(&bundle.program)?;
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "analysis": analysis.to_json(),
                "review": render_rust_review(&bundle.program, &analysis).to_json(),
            }))
        }
        [command, root, export, arguments_path] if command == "run-bundle" => {
            let values = read_arguments_file(arguments_path, "bundle")?;
            let bundle = load_bundle(root)?;
            let (result, log_events) = execute_cli_program(&bundle.program, export, values)?;
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "logEvents": log_events,
                "result": result.to_json()?,
            }))
        }
        [command, path] if command == "format" => {
            let source = read_bounded_program_source(path)?;
            let formatted = format_source(&source, FormatOptions::default())?;
            Ok(json!({
                "ok": true,
                "path": path,
                "changed": formatted.changed,
                "formatterVersion": FORMATTER_VERSION,
                "formattedSource": formatted.source,
            }))
        }
        [command, path, option] if command == "format" && option == "--check" => {
            let source = read_bounded_program_source(path)?;
            let formatted = format_source(&source, FormatOptions::default())?;
            if formatted.changed {
                return Err(Diagnostic::new(
                    "FORMAT_REQUIRED",
                    "source is not in canonical formatter layout",
                    json!({ "path": path, "formatterVersion": FORMATTER_VERSION }),
                ));
            }
            Ok(json!({
                "ok": true,
                "path": path,
                "changed": false,
                "formatterVersion": FORMATTER_VERSION,
            }))
        }
        [command, _, option] if command == "format" => Err(Diagnostic::new(
            "CLI_INVALID_OPTION",
            "the only format option is --check",
            json!({ "option": option }),
        )),
        [command, path] if matches!(command.as_str(), "check" | "inspect") => {
            let source = fs::read_to_string(path).map_err(|error| {
                Diagnostic::new(
                    "HOST_FILE_READ",
                    "host could not read the source file",
                    json!({ "path": path, "kind": error.kind().to_string() }),
                )
            })?;
            let program = load_program_source(&source)?;
            let mut document = json!({ "ok": true, "program": program.inspect_json() });
            if program.version.to_string() == "4" {
                document["analysis"] = analyze_program(&program)?.to_json();
            }
            Ok(document)
        }
        [command, path] if command == "review" => {
            let source = fs::read_to_string(path).map_err(|error| {
                Diagnostic::new(
                    "HOST_FILE_READ",
                    "host could not read the source file",
                    json!({ "path": path, "kind": error.kind().to_string() }),
                )
            })?;
            let program = load_program_source(&source)?;
            let analysis = analyze_program(&program)?;
            Ok(json!({
                "ok": true,
                "analysis": analysis.to_json(),
                "review": render_rust_review(&program, &analysis).to_json(),
            }))
        }
        [command, path] if command == "conformance" => {
            let report = run_manifest(path)?;
            let passed = report.get("passed").and_then(Value::as_bool) == Some(true);
            Ok(json!({ "ok": passed, "report": report }))
        }
        [command, program_path, suite_path] if command == "test-service" => {
            let source = fs::read_to_string(program_path).map_err(|error| {
                Diagnostic::new(
                    "HOST_FILE_READ",
                    "host could not read the source file",
                    json!({ "path": program_path, "kind": error.kind().to_string() }),
                )
            })?;
            let program = load_program_source(&source)?;
            let report = run_service_suite(&program, suite_path)?;
            let passed = report.get("passed").and_then(Value::as_bool) == Some(true);
            Ok(json!({ "ok": passed, "report": report }))
        }
        [command, program_path, suite_path, store_path] if command == "deploy-service" => {
            deploy_service(program_path, suite_path, store_path)
        }
        [command, store_path, suite_path] if command == "evolve-service" => {
            let provider = configured_evolution_provider()?;
            evolve_service_with_provider(store_path, suite_path, false, None, provider.as_ref())
        }
        [command, store_path, suite_path, option] if command == "evolve-service" => {
            if option != "--promote" {
                return Err(Diagnostic::simple(
                    "CLI_INVALID_OPTION",
                    "the only evolve-service option is --promote",
                ));
            }
            let provider = configured_evolution_provider()?;
            evolve_service_with_provider(store_path, suite_path, true, None, provider.as_ref())
        }
        [command, store_path, suite_path, task_option, task_path]
            if command == "evolve-service" && task_option == "--task" =>
        {
            let objective = read_agent_task(task_path)?;
            let provider = configured_evolution_provider()?;
            evolve_service_with_provider(
                store_path,
                suite_path,
                false,
                Some(objective),
                provider.as_ref(),
            )
        }
        [
            command,
            store_path,
            suite_path,
            task_option,
            task_path,
            promote_option,
        ] if command == "evolve-service"
            && task_option == "--task"
            && promote_option == "--promote" =>
        {
            let objective = read_agent_task(task_path)?;
            let provider = configured_evolution_provider()?;
            evolve_service_with_provider(
                store_path,
                suite_path,
                true,
                Some(objective),
                provider.as_ref(),
            )
        }
        [command, initial_path, candidate_path] if command == "version-conformance" => {
            let report = run_version_scenario(initial_path, candidate_path)?;
            let passed = report.get("passed").and_then(Value::as_bool) == Some(true);
            Ok(json!({ "ok": passed, "report": report }))
        }
        [command, code_store, data_store, destination] if command == "backup-service" => {
            create_backup(code_store, data_store, destination)
        }
        [command, snapshot] if command == "verify-backup" => verify_backup(snapshot),
        [command, snapshot, code_store, data_store] if command == "restore-service" => {
            restore_backup(snapshot, code_store, data_store)
        }
        _ => Err(Diagnostic::new(
            "CLI_USAGE",
            "arguments do not match a supported Rust host command",
            json!({ "usage": [
                "compile-bytecode <program.yan> <artifact.ybc.json>",
                "inspect-bytecode <program.yan> <artifact.ybc.json>",
                "run-bytecode <program.yan> <artifact.ybc.json> <export> <arguments.json>",
                "compile-wasm <program.yan> <artifact.wasm>",
                "inspect-wasm <program.yan> <artifact.wasm>",
                "run-wasm <program.yan> <artifact.wasm> <export> <arguments.json>",
                "package-compile <store> <yanshu.lock.json> <artifact.ybc.json> <artifact.wasm>",
                "package-run-compiled <store> <yanshu.lock.json> <artifact.wasm> <export> <arguments.json>",
                "compile-bundle <directory> <artifact.ybc.json> <artifact.wasm>",
                "run-bundle-compiled <directory> <artifact.wasm> <export> <arguments.json>",
                "package-pack <workspace> <store>",
                "package-lock <workspace> <store> <yanshu.lock.json>",
                "package-verify <store> <content-hash>",
                "package-inspect <store> <yanshu.lock.json>",
                "package-review <store> <yanshu.lock.json> [--text]",
                "package-run <store> <yanshu.lock.json> <export> <arguments.json>",
                "seal-bundle <directory> <entry> <module.yan>...",
                "inspect-bundle <directory>",
                "review-bundle <directory>",
                "review-bundle <directory> --text",
                "run-bundle <directory> <export> <arguments.json>",
                "format <program.yan> [--check]",
                "check <program.yan>",
                "inspect <program.yan>",
                "review <program.yan>",
                "review <program.yan> --text",
                "conformance <manifest.json>",
                "test-service <program.yan> <scenarios.json>",
                "deploy-service <program.yan> <scenarios.json> <code-store>",
                "evolve-service <code-store> <scenarios.json> [--task <task.md>] [--promote]",
                "version-conformance <initial.yan> <candidate.yan>",
                "backup-service <code-store> <data-store.json> <snapshot-dir>",
                "verify-backup <snapshot-dir>",
                "restore-service <snapshot-dir> <code-store> <data-store.json>"
            ] }),
        )),
    }
}

fn read_bounded_program_source(path: &str) -> YanshuResult<String> {
    let maximum = yanshu_syntax::ReaderLimits::default().max_source_bytes;
    let file = fs::File::open(path).map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            "host could not open the program source file",
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            "host could not inspect the program source file",
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    if !metadata.is_file() || metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(Diagnostic::new(
            "FORMAT_SOURCE_LIMIT",
            "program source is not a regular file within the formatter byte limit",
            json!({ "path": path, "maximum": maximum }),
        ));
    }

    let read_limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut source = String::new();
    file.take(read_limit)
        .read_to_string(&mut source)
        .map_err(|error| {
            Diagnostic::new(
                "HOST_FILE_READ",
                "host could not read the program source file as UTF-8",
                json!({ "path": path, "kind": error.kind().to_string() }),
            )
        })?;
    if source.len() > maximum {
        return Err(Diagnostic::new(
            "FORMAT_SOURCE_LIMIT",
            "program source exceeds the formatter byte limit",
            json!({ "path": path, "actual": source.len(), "maximum": maximum }),
        ));
    }
    Ok(source)
}

fn read_program_file(path: &str) -> YanshuResult<Program> {
    let source = fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            "host could not read the program source file",
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    load_program_source(&source)
}

fn read_artifact_file(path: &str, artifact: &str) -> YanshuResult<Vec<u8>> {
    fs::read(path).map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            format!("host could not read the {artifact} artifact"),
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })
}

fn read_arguments_file(path: &str, artifact: &str) -> YanshuResult<Vec<yanshu_runtime::Value>> {
    let source = fs::read_to_string(path).map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            format!("host could not read the {artifact} arguments file"),
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    let document: Value = serde_json::from_str(&source).map_err(|error| {
        Diagnostic::new(
            "CLI_ARGUMENTS_JSON",
            format!("{artifact} arguments file is not valid JSON"),
            json!({ "line": error.line(), "column": error.column() }),
        )
    })?;
    let arguments = document.as_array().ok_or_else(|| {
        Diagnostic::simple(
            "CLI_ARGUMENTS_SHAPE",
            format!("{artifact} arguments file must contain one JSON array"),
        )
    })?;
    arguments.iter().map(json_to_value).collect()
}

fn deploy_service(program_path: &str, suite_path: &str, store_path: &str) -> YanshuResult<Value> {
    let source = fs::read_to_string(program_path).map_err(|error| {
        Diagnostic::new(
            "HOST_FILE_READ",
            "host could not read the source file",
            json!({ "path": program_path, "kind": error.kind().to_string() }),
        )
    })?;
    let program = load_program_source(&source)?;
    let report = run_service_suite(&program, suite_path)?;
    let passed = report.get("passed").and_then(Value::as_bool) == Some(true);
    let candidate = source_hash(&source);
    let store = VersionStore::new(store_path);
    let current = store.active_hash()?;
    if current.as_deref() == Some(candidate.as_str()) {
        return Ok(json!({
            "ok": passed,
            "store": store_path,
            "candidate": candidate,
            "report": report,
            "alreadyActive": true,
            "promoted": false,
            "active": current,
        }));
    }

    let provider_metadata = json!({});
    let registered_at = current_seconds();
    let registered = store.register_candidate(CandidateRegistration {
        source: &source,
        parent: current.as_deref(),
        provider: "manual-deploy",
        provider_metadata: &provider_metadata,
        report: &report,
        registered_at,
    })?;
    let promoted = if passed {
        store.promote(&registered, current_seconds())?;
        true
    } else {
        false
    };
    let active = store.active_hash()?;
    Ok(json!({
        "ok": passed,
        "store": store_path,
        "candidate": registered,
        "report": report,
        "alreadyActive": false,
        "promoted": promoted,
        "active": active,
    }))
}

fn current_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn read_agent_task(path: &str) -> YanshuResult<String> {
    const MAXIMUM_TASK_BYTES: u64 = 64 * 1024;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        Diagnostic::new(
            "CLI_TASK_READ",
            "agent task file could not be read",
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(Diagnostic::simple(
            "CLI_TASK_INVALID_FILE",
            "agent task must be a regular file",
        ));
    }
    if metadata.len() > MAXIMUM_TASK_BYTES {
        return Err(Diagnostic::new(
            "CLI_TASK_TOO_LARGE",
            "agent task exceeded the byte limit",
            json!({ "limitBytes": MAXIMUM_TASK_BYTES }),
        ));
    }
    let mut file = fs::File::open(path).map_err(|error| {
        Diagnostic::new(
            "CLI_TASK_READ",
            "agent task file could not be opened",
            json!({ "path": path, "kind": error.kind().to_string() }),
        )
    })?;
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAXIMUM_TASK_BYTES as usize));
    Read::take(&mut file, MAXIMUM_TASK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Diagnostic::new(
                "CLI_TASK_READ",
                "agent task file could not be read",
                json!({ "path": path, "kind": error.kind().to_string() }),
            )
        })?;
    if bytes.len() as u64 > MAXIMUM_TASK_BYTES {
        return Err(Diagnostic::new(
            "CLI_TASK_TOO_LARGE",
            "agent task exceeded the byte limit",
            json!({ "limitBytes": MAXIMUM_TASK_BYTES }),
        ));
    }
    let objective = String::from_utf8(bytes).map_err(|_| {
        Diagnostic::new(
            "CLI_TASK_READ",
            "agent task must be valid UTF-8 text",
            json!({ "path": path }),
        )
    })?;
    if objective.trim().is_empty() {
        return Err(Diagnostic::simple(
            "CLI_TASK_EMPTY",
            "agent task must not be empty",
        ));
    }
    Ok(objective)
}

fn evolve_service_with_provider(
    store_path: &str,
    suite_path: &str,
    promotion_requested: bool,
    objective: Option<String>,
    provider: &dyn EvolutionProvider,
) -> YanshuResult<Value> {
    let store = VersionStore::new(store_path);
    let current_hash = store.active_hash()?.ok_or_else(|| {
        Diagnostic::simple("VERSION_NO_ACTIVE", "version store has no active version")
    })?;
    let current_source = store.version_source(&current_hash)?;
    let current_program = load_program_source(&current_source)?;
    let current_report = run_service_suite(&current_program, suite_path)?;
    let proposal = provider.propose(&EvolutionRequest {
        current_hash: current_hash.clone(),
        current_source,
        observations: current_report.clone(),
        objective,
    })?;
    let candidate_program = load_program_source(&proposal.source)?;
    let candidate_report = run_service_suite(&candidate_program, suite_path)?;
    let passed = candidate_report.get("passed").and_then(Value::as_bool) == Some(true);
    let candidate_hash = source_hash(&proposal.source);
    let already_active = candidate_hash == current_hash;
    let registered = if already_active {
        candidate_hash
    } else {
        store.register_candidate(CandidateRegistration {
            source: &proposal.source,
            parent: Some(&current_hash),
            provider: proposal.provider,
            provider_metadata: &proposal.metadata,
            report: &candidate_report,
            registered_at: current_seconds(),
        })?
    };
    let promoted = if promotion_requested && passed && !already_active {
        store.promote(&registered, current_seconds())?;
        true
    } else {
        false
    };
    Ok(json!({
        "ok": passed,
        "store": store_path,
        "current": {
            "hash": current_hash,
            "report": current_report,
        },
        "candidate": {
            "hash": registered,
            "provider": proposal.provider,
            "notes": proposal.notes,
            "report": candidate_report,
            "alreadyActive": already_active,
        },
        "promotionRequested": promotion_requested,
        "promoted": promoted,
        "active": store.active_hash()?,
    }))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::{Value, json};
    use yanshu_diagnostic::{Diagnostic, YanshuResult};
    use yanshu_provider::FileProvider;

    use super::run;

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn require_error(result: YanshuResult<Value>) -> Diagnostic {
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

        let misplaced_text = require_error(run(vec!["review".to_owned(), "--text".to_owned()]));
        assert_eq!(misplaced_text.code, "CLI_INVALID_OPTION");
    }

    #[test]
    fn formats_programs_as_json_and_checks_canonical_layout() {
        let temporary = TestDirectory::new();
        let program = temporary.path.join("compact.yan");
        fs::write(
            &program,
            b"(program (name cli-format) (version 1) (def value (fn () 1)) (export value))",
        )
        .unwrap_or_else(|error| panic!("format fixture failed: {error}"));

        let rendered = run(vec!["format".to_owned(), program.display().to_string()])
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(rendered["ok"], true);
        assert_eq!(rendered["changed"], true);
        assert_eq!(rendered["formatterVersion"], 1);
        let canonical = rendered["formattedSource"]
            .as_str()
            .unwrap_or_else(|| panic!("formatted source was not a string"));

        let required = require_error(run(vec![
            "format".to_owned(),
            program.display().to_string(),
            "--check".to_owned(),
        ]));
        assert_eq!(required.code, "FORMAT_REQUIRED");

        fs::write(&program, canonical)
            .unwrap_or_else(|error| panic!("canonical fixture failed: {error}"));
        let checked = run(vec![
            "format".to_owned(),
            program.display().to_string(),
            "--check".to_owned(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(checked["ok"], true);
        assert_eq!(checked["changed"], false);
    }

    #[test]
    fn reviews_and_runs_a_typed_sealed_bundle() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let bundle = project_root.join("examples/bundles/typed-expense");
        let arguments = bundle.join("arguments.json");
        let reviewed = run(vec![
            "review-bundle".to_owned(),
            bundle.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(reviewed["analysis"]["capabilityClosure"], json!(["log"]));
        assert_eq!(reviewed["review"]["editable"], false);
        assert_eq!(reviewed["review"]["renderer"], "rust-readonly-v3");

        let executed = run(vec![
            "run-bundle".to_owned(),
            bundle.display().to_string(),
            "evaluate".to_owned(),
            arguments.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(executed["result"]["status"], "review");
        assert_eq!(executed["result"]["amount"], 1200);

        let bytecode = temporary.path.join("bundle.ybc.json");
        let wasm = temporary.path.join("bundle.wasm");
        let compiled = run(vec![
            "compile-bundle".to_owned(),
            bundle.display().to_string(),
            bytecode.display().to_string(),
            wasm.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(compiled["bundleHash"], reviewed["bundleHash"]);
        let compiled_result = run(vec![
            "run-bundle-compiled".to_owned(),
            bundle.display().to_string(),
            wasm.display().to_string(),
            "evaluate".to_owned(),
            arguments.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(compiled_result["result"], executed["result"]);
    }

    #[test]
    fn locks_reviews_and_runs_a_content_addressed_package() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let workspace = project_root.join("examples/packages/typed-expense");
        let store = temporary.path.join("packages");
        let lock_path = temporary.path.join("yanshu.lock.json");
        let locked = run(vec![
            "package-lock".to_owned(),
            workspace.display().to_string(),
            store.display().to_string(),
            lock_path.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(locked["lock"]["packages"].as_array().map(Vec::len), Some(2));
        assert_eq!(locked["lock"]["capabilityClosure"], json!(["log"]));

        let reviewed = run(vec![
            "package-review".to_owned(),
            store.display().to_string(),
            lock_path.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(reviewed["review"]["editable"], false);

        let executed = run(vec![
            "package-run".to_owned(),
            store.display().to_string(),
            lock_path.display().to_string(),
            "evaluate".to_owned(),
            workspace.join("arguments.json").display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(executed["result"]["status"], "review");
        assert_eq!(executed["lockHash"], locked["lockHash"]);

        let bytecode = temporary.path.join("typed-expense.ybc.json");
        let wasm = temporary.path.join("typed-expense.wasm");
        let compiled = run(vec![
            "package-compile".to_owned(),
            store.display().to_string(),
            lock_path.display().to_string(),
            bytecode.display().to_string(),
            wasm.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(compiled["ok"], true);
        assert!(
            fs::read(&wasm)
                .unwrap_or_else(|error| panic!("WASM read failed: {error}"))
                .starts_with(b"\0asm")
        );

        let compiled_result = run(vec![
            "package-run-compiled".to_owned(),
            store.display().to_string(),
            lock_path.display().to_string(),
            wasm.display().to_string(),
            "evaluate".to_owned(),
            workspace.join("arguments.json").display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(compiled_result["result"], executed["result"]);
        assert_eq!(compiled_result["lockHash"], locked["lockHash"]);
    }

    #[test]
    fn deploys_only_after_the_service_suite_passes() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let program = project_root.join("examples/tasks/service.yan");
        let suite = project_root.join("examples/tasks/scenarios.json");
        let failing_suite = temporary.path.join("failing-scenarios.json");
        let mut failing_document: Value = serde_json::from_str(
            &fs::read_to_string(&suite)
                .unwrap_or_else(|error| panic!("scenario read failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("scenario JSON failed: {error}"));
        failing_document["cases"][0]["expectStatus"] = json!(418);
        fs::write(
            &failing_suite,
            serde_json::to_vec(&failing_document)
                .unwrap_or_else(|error| panic!("scenario encoding failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("scenario write failed: {error}"));
        let failing_store = temporary.path.join("failing-code");
        let failed = run(vec![
            "deploy-service".to_owned(),
            program.display().to_string(),
            failing_suite.display().to_string(),
            failing_store.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["promoted"], false);
        assert_eq!(failed["active"], Value::Null);

        let store = temporary.path.join("code");
        let arguments = vec![
            "deploy-service".to_owned(),
            program.display().to_string(),
            suite.display().to_string(),
            store.display().to_string(),
        ];

        let first = run(arguments.clone()).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(first["ok"], true);
        assert_eq!(first["promoted"], true);
        assert_eq!(first["alreadyActive"], false);

        let second = run(arguments).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(second["ok"], true);
        assert_eq!(second["promoted"], false);
        assert_eq!(second["alreadyActive"], true);
    }

    #[test]
    fn service_evolution_registers_before_explicit_promotion() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let program = project_root.join("examples/tasks/service.yan");
        let suite = project_root.join("examples/tasks/scenarios.json");
        let store = temporary.path.join("code");
        let deployed = run(vec![
            "deploy-service".to_owned(),
            program.display().to_string(),
            suite.display().to_string(),
            store.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let original = deployed["active"]
            .as_str()
            .unwrap_or_else(|| panic!("deployment must have an active hash"))
            .to_owned();
        let candidate_path = temporary.path.join("candidate.yan");
        let candidate_source = format!(
            "; candidate keeps behavior while exercising the evolution gate\n{}",
            fs::read_to_string(&program)
                .unwrap_or_else(|error| panic!("program read failed: {error}"))
        );
        fs::write(&candidate_path, candidate_source)
            .unwrap_or_else(|error| panic!("candidate write failed: {error}"));
        let provider = FileProvider::new(&candidate_path);

        let staged = super::evolve_service_with_provider(
            &store.display().to_string(),
            &suite.display().to_string(),
            false,
            None,
            &provider,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(staged["ok"], true);
        assert_eq!(staged["promoted"], false);
        assert_eq!(staged["active"], original);

        let promoted = super::evolve_service_with_provider(
            &store.display().to_string(),
            &suite.display().to_string(),
            true,
            None,
            &provider,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(promoted["ok"], true);
        assert_eq!(promoted["promoted"], true);
        assert_eq!(promoted["active"], promoted["candidate"]["hash"]);
        assert_ne!(promoted["active"], original);
    }

    #[test]
    fn agent_task_input_is_bounded_nonempty_utf8() {
        let temporary = TestDirectory::new();
        let task = temporary.path.join("task.md");
        fs::write(&task, "add an explicit duplicate-title rule")
            .unwrap_or_else(|error| panic!("task write failed: {error}"));
        assert_eq!(
            super::read_agent_task(&task.display().to_string())
                .unwrap_or_else(|error| panic!("task read failed: {error}")),
            "add an explicit duplicate-title rule"
        );

        fs::write(&task, "  \n").unwrap_or_else(|error| panic!("empty task write failed: {error}"));
        let error = super::read_agent_task(&task.display().to_string())
            .err()
            .unwrap_or_else(|| panic!("empty task unexpectedly passed"));
        assert_eq!(error.code, "CLI_TASK_EMPTY");

        fs::write(&task, vec![b'x'; 64 * 1024 + 1])
            .unwrap_or_else(|error| panic!("oversized task write failed: {error}"));
        let error = super::read_agent_task(&task.display().to_string())
            .err()
            .unwrap_or_else(|| panic!("oversized task unexpectedly passed"));
        assert_eq!(error.code, "CLI_TASK_TOO_LARGE");
    }

    #[test]
    fn backup_commands_verify_and_restore_a_deployed_service() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let program = project_root.join("examples/tasks/service.yan");
        let suite = project_root.join("examples/tasks/scenarios.json");
        let code = temporary.path.join("code");
        let data = temporary.path.join("data.json");
        let snapshot = temporary.path.join("snapshot");
        let restored_code = temporary.path.join("restored-code");
        let restored_data = temporary.path.join("restored-data.json");
        run(vec![
            "deploy-service".to_owned(),
            program.display().to_string(),
            suite.display().to_string(),
            code.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        fs::write(&data, b"{\"version\":1,\"entries\":[]}\n")
            .unwrap_or_else(|error| panic!("data fixture failed: {error}"));

        let backup = run(vec![
            "backup-service".to_owned(),
            code.display().to_string(),
            data.display().to_string(),
            snapshot.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(backup["ok"], true);
        let verified = run(vec![
            "verify-backup".to_owned(),
            snapshot.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(verified["activeVersion"], backup["activeVersion"]);
        let restored = run(vec![
            "restore-service".to_owned(),
            snapshot.display().to_string(),
            restored_code.display().to_string(),
            restored_data.display().to_string(),
        ])
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            restored["restored"]["activeVersion"],
            backup["activeVersion"]
        );
        assert_eq!(
            fs::read(restored_data)
                .unwrap_or_else(|error| panic!("restored data read failed: {error}")),
            fs::read(data).unwrap_or_else(|error| panic!("source data read failed: {error}"))
        );
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            for _ in 0..32 {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_nanos());
                let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "ai-lang-rust-cli-{}-{nonce}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("temporary directory failed: {error}"),
                }
            }
            panic!("temporary directory name could not be reserved")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}
