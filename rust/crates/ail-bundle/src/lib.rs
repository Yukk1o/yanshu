#![forbid(unsafe_code)]

mod graph;
mod linker;
mod manifest;

pub use manifest::{
    BundleManifest, LoadedBundle, ModuleManifest, load_bundle, seal_bundle_directory,
};

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use ail_diagnostic::AilResult;
    use ail_runtime::{ExecutionOptions, Value, execute_export};

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

    fn require<T>(result: AilResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("ail-bundle-{label}-{}-{nonce}", std::process::id()))
    }

    fn fixture(label: &str, app: &str, math: &str) -> PathBuf {
        let root = temporary_directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("app.ail"), app).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("math.ail"), math).unwrap_or_else(|error| panic!("{error}"));
        root
    }

    #[test]
    fn seals_links_and_executes_a_multi_module_bundle() {
        let root = fixture("execute", APP, MATH);
        let manifest = require(seal_bundle_directory(
            &root,
            "app",
            &["math.ail".to_owned(), "app.ail".to_owned()],
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
            &["app.ail".to_owned(), "math.ail".to_owned()],
        ));
        fs::write(tampered.join("math.ail"), MATH.replace("twice", "double"))
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
            &["app.ail".to_owned(), "math.ail".to_owned()],
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected cycle diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_IMPORT_CYCLE");

        let diagnostic = match seal_bundle_directory(&cycle, "app", &["../escape.ail".to_owned()]) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected path diagnostic"),
        };
        assert_eq!(diagnostic.code, "BUNDLE_INVALID_MODULE_PATH");
        fs::remove_dir_all(cycle).unwrap_or_else(|error| panic!("{error}"));
    }
}
