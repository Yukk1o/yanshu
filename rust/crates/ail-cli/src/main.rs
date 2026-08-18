#![forbid(unsafe_code)]

use std::{
    env, fs,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use ail_bundle::{load_bundle, seal_bundle_directory};
use ail_conformance::run_manifest;
use ail_diagnostic::{AilResult, Diagnostic};
use ail_ops::{create_backup, restore_backup, verify_backup};
use ail_provider::{EvolutionProvider, EvolutionRequest, configured_live_provider};
use ail_runtime::{ExecutionOptions, execute_export, json_to_value};
use ail_service::run_service_suite;
use ail_store::{CandidateRegistration, VersionStore, run_version_scenario, source_hash};
use ail_syntax::load_program_source;
use serde_json::{Value, json};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(document) => {
            println!("{document}");
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

fn run(arguments: Vec<String>) -> AilResult<Value> {
    match arguments.as_slice() {
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
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "manifest": bundle.manifest.to_json(),
                "program": bundle.program.inspect_json(),
            }))
        }
        [command, root, export, arguments_path] if command == "run-bundle" => {
            let source = fs::read_to_string(arguments_path).map_err(|error| {
                Diagnostic::new(
                    "HOST_FILE_READ",
                    "host could not read the bundle arguments file",
                    json!({ "path": arguments_path, "kind": error.kind().to_string() }),
                )
            })?;
            let document: Value = serde_json::from_str(&source).map_err(|error| {
                Diagnostic::new(
                    "CLI_ARGUMENTS_JSON",
                    "bundle arguments file is not valid JSON",
                    json!({ "line": error.line(), "column": error.column() }),
                )
            })?;
            let arguments = document.as_array().ok_or_else(|| {
                Diagnostic::simple(
                    "CLI_ARGUMENTS_SHAPE",
                    "bundle arguments file must contain one JSON array",
                )
            })?;
            let values = arguments
                .iter()
                .map(json_to_value)
                .collect::<AilResult<Vec<_>>>()?;
            let bundle = load_bundle(root)?;
            let result =
                execute_export(&bundle.program, export, values, ExecutionOptions::default())?;
            Ok(json!({
                "ok": true,
                "bundleHash": bundle.bundle_hash,
                "result": result.to_json()?,
            }))
        }
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
            let provider = configured_live_provider()?;
            evolve_service_with_provider(store_path, suite_path, false, &provider)
        }
        [command, store_path, suite_path, option] if command == "evolve-service" => {
            if option != "--promote" {
                return Err(Diagnostic::simple(
                    "CLI_INVALID_OPTION",
                    "the only evolve-service option is --promote",
                ));
            }
            let provider = configured_live_provider()?;
            evolve_service_with_provider(store_path, suite_path, true, &provider)
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
                "seal-bundle <directory> <entry> <module.ail>...",
                "inspect-bundle <directory>",
                "run-bundle <directory> <export> <arguments.json>",
                "check <program.ail>",
                "inspect <program.ail>",
                "conformance <manifest.json>",
                "test-service <program.ail> <scenarios.json>",
                "deploy-service <program.ail> <scenarios.json> <code-store>",
                "evolve-service <code-store> <scenarios.json> [--promote]",
                "version-conformance <initial.ail> <candidate.ail>",
                "backup-service <code-store> <data-store.json> <snapshot-dir>",
                "verify-backup <snapshot-dir>",
                "restore-service <snapshot-dir> <code-store> <data-store.json>"
            ] }),
        )),
    }
}

fn deploy_service(program_path: &str, suite_path: &str, store_path: &str) -> AilResult<Value> {
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

fn evolve_service_with_provider(
    store_path: &str,
    suite_path: &str,
    promotion_requested: bool,
    provider: &dyn EvolutionProvider,
) -> AilResult<Value> {
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
        time::{SystemTime, UNIX_EPOCH},
    };

    use ail_diagnostic::{AilResult, Diagnostic};
    use ail_provider::FileProvider;
    use serde_json::{Value, json};

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

    #[test]
    fn deploys_only_after_the_service_suite_passes() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let program = project_root.join("examples/tasks/service.ail");
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
        let program = project_root.join("examples/tasks/service.ail");
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
        let candidate_path = temporary.path.join("candidate.ail");
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
            &provider,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(promoted["ok"], true);
        assert_eq!(promoted["promoted"], true);
        assert_eq!(promoted["active"], promoted["candidate"]["hash"]);
        assert_ne!(promoted["active"], original);
    }

    #[test]
    fn backup_commands_verify_and_restore_a_deployed_service() {
        let temporary = TestDirectory::new();
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let program = project_root.join("examples/tasks/service.ail");
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
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir()
                .join(format!("ai-lang-rust-cli-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}
