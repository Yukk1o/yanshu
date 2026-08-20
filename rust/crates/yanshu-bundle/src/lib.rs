#![forbid(unsafe_code)]

mod graph;
mod linker;
mod manifest;

use std::collections::BTreeMap;

use yanshu_diagnostic::YanshuResult;
use yanshu_syntax::Program;

pub use manifest::{
    BundleManifest, LoadedBundle, ModuleManifest, load_bundle, parse_bundle_manifest_bytes,
    seal_bundle_directory,
};

pub fn link_program_set(
    programs: &BTreeMap<String, Program>,
    entry: &str,
) -> YanshuResult<Program> {
    let order = graph::dependency_order(programs, entry)?;
    linker::link_programs(programs, &order, entry)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use yanshu_diagnostic::YanshuResult;
    use yanshu_runtime::{ExecutionOptions, Value, execute_export};

    use crate::{load_bundle, seal_bundle_directory};

    const MATH: &str = r#"(program
      (name math)
      (version 3)
      (data maybe (some value) (none))
      (def twice (fn (value) (+ value value)))
      (export twice some none))"#;

    const APP: &str = r#"(program
      (name app)
      (version 3)
      (imports math)
      (def run (fn (value)
        (match (some (twice value))
          ((some result) result)
          (_ 0))))
      (export run))"#;

    const V4_POLICY: &str = r#"(program
      (name policy)
      (version 4)
      (capabilities log)
      (data decision (approved (amount integer)))
      (export-types decision)
      (signature decide (fn (integer) decision))
      (def decide (fn (amount) (do (log amount) (approved amount))))
      (export decide approved))"#;

    const V4_APP: &str = r#"(program
      (name app)
      (version 4)
      (imports policy)
      (signature extract (fn (decision) integer))
      (def extract (fn (decision)
        (match decision
          ((approved value) value)
          (_ 0))))
      (signature run (fn (integer) integer))
      (def run (fn (amount) (extract (decide amount))))
      (export run))"#;

    fn require<T>(result: YanshuResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "yanshu-bundle-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn fixture(label: &str, app: &str, math: &str) -> PathBuf {
        let root = temporary_directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("app.yan"), app).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("math.yan"), math).unwrap_or_else(|error| panic!("{error}"));
        root
    }

    #[test]
    fn seals_links_and_executes_a_multi_module_bundle() {
        let root = fixture("execute", APP, MATH);
        let manifest = require(seal_bundle_directory(
            &root,
            "app",
            &["math.yan".to_owned(), "app.yan".to_owned()],
        ));
        assert_eq!(manifest.modules[0].name, "app");
        assert_eq!(manifest.content_hash().len(), 64);

        let bundle = require(load_bundle(&root));
        assert!(bundle.program.imports.is_empty());
        assert_eq!(bundle.bundle_hash, manifest.content_hash());
        assert_eq!(
            require(execute_export(
                &bundle.program,
                "run",
                vec![Value::Int(21.into())],
                ExecutionOptions::default(),
            )),
            Value::Int(42.into())
        );
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn detects_tampering_cycles_and_path_escape() {
        let tampered = fixture("tamper", APP, MATH);
        require(seal_bundle_directory(
            &tampered,
            "app",
            &["app.yan".to_owned(), "math.yan".to_owned()],
        ));
        fs::write(tampered.join("math.yan"), MATH.replace("twice", "double"))
            .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match load_bundle(&tampered) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected tamper diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_MODULE_HASH_MISMATCH");
        fs::remove_dir_all(tampered).unwrap_or_else(|error| panic!("{error}"));

        let cycle = fixture(
            "cycle",
            APP,
            &MATH.replace("(version 3)", "(version 3) (imports app)"),
        );
        let diagnostic = match seal_bundle_directory(
            &cycle,
            "app",
            &["app.yan".to_owned(), "math.yan".to_owned()],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected cycle diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_IMPORT_CYCLE");

        let diagnostic = match seal_bundle_directory(&cycle, "app", &["../escape.yan".to_owned()]) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected path diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_INVALID_MODULE_PATH");
        fs::remove_dir_all(cycle).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn rejects_oversized_manifest_and_module_files_before_allocating_them() {
        let manifest_root = temporary_directory("manifest-limit");
        fs::create_dir_all(&manifest_root).unwrap_or_else(|error| panic!("{error}"));
        let manifest = fs::File::create(manifest_root.join("bundle.json"))
            .unwrap_or_else(|error| panic!("{error}"));
        manifest
            .set_len(super::manifest::MAXIMUM_MANIFEST_BYTES + 1)
            .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match load_bundle(&manifest_root) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("oversized manifest must fail closed"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_MANIFEST_LIMIT");
        fs::remove_dir_all(manifest_root).unwrap_or_else(|error| panic!("{error}"));

        let module_root = temporary_directory("module-limit");
        fs::create_dir_all(&module_root).unwrap_or_else(|error| panic!("{error}"));
        let module = fs::File::create(module_root.join("large.yan"))
            .unwrap_or_else(|error| panic!("{error}"));
        module
            .set_len(super::manifest::MAXIMUM_MODULE_BYTES + 1)
            .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic =
            match seal_bundle_directory(&module_root, "large", &["large.yan".to_owned()]) {
                Err(diagnostic) => diagnostic,
                Ok(_) => panic!("oversized module must fail closed"),
            };
        assert_eq!(diagnostic.code, "BUNDLE_MODULE_LIMIT");
        fs::remove_dir_all(module_root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn seals_and_rechecks_v4_capability_closure() {
        let root = fixture("v4-effects", V4_APP, V4_POLICY);
        let manifest = require(seal_bundle_directory(
            &root,
            "app",
            &["app.yan".to_owned(), "math.yan".to_owned()],
        ));
        assert_eq!(manifest.format_version, 2);
        assert_eq!(manifest.capability_closure, Some(vec!["log".to_owned()]));
        let loaded = require(load_bundle(&root));
        assert_eq!(
            loaded.manifest.capability_closure,
            Some(vec!["log".to_owned()])
        );
        let imported_signature = loaded
            .program
            .signatures
            .iter()
            .find(|signature| signature.name == "app/extract")
            .unwrap_or_else(|| panic!("linked imported-type signature is missing"));
        assert_eq!(
            imported_signature.parameters[0].to_json(),
            serde_json::json!({ "type": "named", "name": "policy/decision" })
        );

        let manifest_path = root.join("bundle.json");
        let mut document: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&manifest_path).unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        document["capabilityClosure"] = serde_json::json!([]);
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&document).unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match load_bundle(&root) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected capability closure diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_CAPABILITY_CLOSURE_MISMATCH");
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn rejects_unknown_and_ambiguous_imported_types() {
        let unknown = fixture(
            "unknown-imported-type",
            V4_APP,
            &V4_POLICY.replace("      (export-types decision)\n", ""),
        );
        let diagnostic = match seal_bundle_directory(
            &unknown,
            "app",
            &["app.yan".to_owned(), "math.yan".to_owned()],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected unknown imported type diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_UNKNOWN_IMPORTED_TYPE");
        fs::remove_dir_all(unknown).unwrap_or_else(|error| panic!("{error}"));

        let ambiguous = temporary_directory("ambiguous-imported-type");
        fs::create_dir_all(&ambiguous).unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            ambiguous.join("left.yan"),
            "(program (name left) (version 4) (data decision (left-approved (amount integer))) (export-types decision) (export left-approved))",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            ambiguous.join("right.yan"),
            "(program (name right) (version 4) (data decision (right-approved (amount integer))) (export-types decision) (export right-approved))",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            ambiguous.join("app.yan"),
            "(program (name app) (version 4) (imports left right) (signature run (fn (decision) integer)) (def run (fn (value) 1)) (export run))",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match seal_bundle_directory(
            &ambiguous,
            "app",
            &[
                "app.yan".to_owned(),
                "left.yan".to_owned(),
                "right.yan".to_owned(),
            ],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected ambiguous imported type diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_AMBIGUOUS_IMPORTED_TYPE");
        fs::remove_dir_all(ambiguous).unwrap_or_else(|error| panic!("{error}"));
    }
}
