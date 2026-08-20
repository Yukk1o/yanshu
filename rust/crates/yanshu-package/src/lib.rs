#![forbid(unsafe_code)]

mod format;
mod model;
mod parse;
mod store;

pub use model::{
    LoadedPackage, LockedPackage, PackageDependency, PackageLock, PackageManifest, PackageModule,
    SourceDependency, SourceDescriptor,
};
pub use store::{load_locked_package, lock_workspace, pack_workspace, verify_package};

use yanshu_diagnostic::YanshuResult;

/// Parses an untrusted package source descriptor with the production byte and
/// structural limits, without reading from the filesystem.
pub fn parse_package_source_bytes(bytes: &[u8]) -> YanshuResult<SourceDescriptor> {
    parse::source_descriptor(&parse::document(bytes, "package source descriptor")?)
}

/// Parses an untrusted content-addressed package manifest with the production
/// byte and structural limits, without reading from the filesystem.
pub fn parse_package_manifest_bytes(bytes: &[u8]) -> YanshuResult<PackageManifest> {
    parse::package_manifest(&parse::document(bytes, "package manifest")?)
}

/// Parses an untrusted package lock with the production byte and structural
/// limits, without reading from the filesystem.
pub fn parse_package_lock_bytes(bytes: &[u8]) -> YanshuResult<PackageLock> {
    parse::package_lock(&parse::document(bytes, "package lock")?)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use yanshu_runtime::{ExecutionOptions, Value, execute_export};

    use crate::{
        load_locked_package, lock_workspace, parse_package_lock_bytes,
        parse_package_manifest_bytes, parse_package_source_bytes,
    };

    const POLICY: &str = r#"(program
      (name policy)
      (version 4)
      (signature twice (fn (integer) integer))
      (def twice (fn (value) (+ value value)))
      (export twice))"#;

    const APP: &str = r#"(program
      (name app)
      (version 4)
      (imports policy)
      (signature run (fn (integer) integer))
      (def run (fn (value) (twice value)))
      (export run))"#;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "yanshu-package-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap_or_else(|error| panic!("{error}"));
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn locks_loads_and_executes_only_content_addressed_sources() {
        let temporary = TestDirectory::new();
        let root = temporary.0.join("workspace");
        let policy = root.join("packages/policy");
        let store = temporary.0.join("store");
        let lock_path = root.join("yanshu.lock.json");
        fs::create_dir_all(&policy).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("app.yan"), APP).unwrap_or_else(|error| panic!("{error}"));
        fs::write(policy.join("policy.yan"), POLICY).unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            root.join("yanshu-package.source.json"),
            r#"{
              "formatVersion": 1,
              "name": "expense-app",
              "version": "1.0.0",
              "entry": "app",
              "modules": ["app.yan"],
              "dependencies": [{"name":"policy-lib","path":"packages/policy"}]
            }"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            policy.join("yanshu-package.source.json"),
            r#"{
              "formatVersion": 1,
              "name": "policy-lib",
              "version": "1.2.0",
              "entry": "policy",
              "modules": ["policy.yan"],
              "dependencies": []
            }"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));

        let lock = lock_workspace(&root, &store, &lock_path)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(lock.packages.len(), 2);
        assert!(lock.capability_closure.is_empty());
        let loaded = load_locked_package(&store, &lock_path)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            execute_export(
                &loaded.program,
                "run",
                vec![Value::Int(21.into())],
                ExecutionOptions::default(),
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}")),
            Value::Int(42.into())
        );

        fs::write(
            policy.join("policy.yan"),
            POLICY.replace("+ value value", "+ value 100"),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let loaded_again = load_locked_package(&store, &lock_path)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(loaded.lock_hash, loaded_again.lock_hash);

        let original_lock = fs::read(&lock_path).unwrap_or_else(|error| panic!("{error}"));
        let mut tampered_lock: serde_json::Value =
            serde_json::from_slice(&original_lock).unwrap_or_else(|error| panic!("{error}"));
        tampered_lock["capabilityClosure"] = serde_json::json!(["log"]);
        fs::write(
            &lock_path,
            serde_json::to_vec_pretty(&tampered_lock).unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match load_locked_package(&store, &lock_path) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected lock mismatch diagnostic"),
        };
        assert_eq!(diagnostic.code, "PACKAGE_LOCK_MISMATCH");
        fs::write(&lock_path, original_lock).unwrap_or_else(|error| panic!("{error}"));

        let policy_hash = lock
            .packages
            .iter()
            .find(|package| package.name == "policy-lib")
            .map(|package| package.content_hash.clone())
            .unwrap_or_else(|| panic!("policy package missing from lock"));
        fs::write(
            store.join("sha256").join(policy_hash).join("policy.yan"),
            "tampered",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match load_locked_package(&store, &lock_path) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected stored module tampering diagnostic"),
        };
        assert_eq!(diagnostic.code, "PACKAGE_MODULE_HASH_MISMATCH");
    }

    #[test]
    fn rejects_source_dependency_path_escape() {
        let temporary = TestDirectory::new();
        let root = temporary.0.join("workspace");
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("app.yan"), APP).unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            root.join("yanshu-package.source.json"),
            r#"{
              "formatVersion": 1,
              "name": "escape-app",
              "version": "1.0.0",
              "entry": "app",
              "modules": ["app.yan"],
              "dependencies": [{"name":"outside","path":"../outside"}]
            }"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let diagnostic = match lock_workspace(
            &root,
            temporary.0.join("store"),
            root.join("yanshu.lock.json"),
        ) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected dependency path diagnostic"),
        };
        assert_eq!(diagnostic.code, "PACKAGE_INVALID_PATH");
    }

    #[test]
    fn byte_parsers_share_the_filesystem_loader_limit() {
        let oversized = vec![b' '; super::parse::MAXIMUM_DOCUMENT_BYTES as usize + 1];
        for diagnostic in [
            parse_package_source_bytes(&oversized).err(),
            parse_package_manifest_bytes(&oversized).err(),
            parse_package_lock_bytes(&oversized).err(),
        ] {
            assert_eq!(
                diagnostic.map(|value| value.code),
                Some("PACKAGE_FILE_LIMIT")
            );
        }

        let descriptor = parse_package_source_bytes(
            br#"{
              "formatVersion": 1,
              "name": "bounded-source",
              "version": "1.0.0",
              "entry": "app",
              "modules": ["app.yan"],
              "dependencies": []
            }"#,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(descriptor.entry, "app");
    }
}
