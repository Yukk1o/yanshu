#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use yanshu_analysis::analyze_program;
use yanshu_bundle::link_program_set;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::{Program, load_program_source};

use crate::{
    format::{sha256, valid_hash},
    model::{
        LoadedPackage, LockedPackage, PackageDependency, PackageLock, PackageManifest,
        PackageModule, SourceDescriptor,
    },
    parse::{package_lock, package_manifest, source_descriptor},
};

const SOURCE_DESCRIPTOR: &str = "yanshu-package.source.json";
const ARTIFACT_MANIFEST: &str = "package.json";
const MAXIMUM_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAXIMUM_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
struct BuiltPackage {
    name: String,
    content_hash: String,
    manifest: PackageManifest,
}

#[derive(Debug)]
struct VerifiedArtifact {
    manifest: PackageManifest,
    programs: BTreeMap<String, Program>,
}

struct BuildContext {
    workspace_root: PathBuf,
    store_root: PathBuf,
    visiting: BTreeSet<PathBuf>,
    built: BTreeMap<PathBuf, BuiltPackage>,
}

pub fn pack_workspace(root: impl AsRef<Path>, store: impl AsRef<Path>) -> YanshuResult<String> {
    let root = canonical_directory(root.as_ref(), "PACKAGE_WORKSPACE_MISSING")?;
    let store = absolute_path(store.as_ref())?;
    fs::create_dir_all(store.join("sha256")).map_err(|error| {
        io_diagnostic(
            "PACKAGE_STORE_CREATE",
            "could not create the package store",
            &store,
            &error,
        )
    })?;
    let mut context = BuildContext {
        workspace_root: root.clone(),
        store_root: store,
        visiting: BTreeSet::new(),
        built: BTreeMap::new(),
    };
    Ok(context.build(&root)?.content_hash.clone())
}

pub fn lock_workspace(
    root: impl AsRef<Path>,
    store: impl AsRef<Path>,
    lock_path: impl AsRef<Path>,
) -> YanshuResult<PackageLock> {
    let store = absolute_path(store.as_ref())?;
    let root_hash = pack_workspace(root, &store)?;
    let (lock, _) = resolve_store(&store, &root_hash)?;
    write_json(lock_path.as_ref(), &lock.to_json(), "PACKAGE_LOCK_WRITE")?;
    Ok(lock)
}

pub fn load_locked_package(
    store: impl AsRef<Path>,
    lock_path: impl AsRef<Path>,
) -> YanshuResult<LoadedPackage> {
    let store = absolute_path(store.as_ref())?;
    let document = read_json(lock_path.as_ref(), MAXIMUM_DOCUMENT_BYTES, "package lock")?;
    let sealed = package_lock(&document)?;
    let (computed, program) = resolve_store(&store, &sealed.root_package)?;
    if sealed != computed {
        return Err(Diagnostic::new(
            "PACKAGE_LOCK_MISMATCH",
            "package lock does not match the verified content-addressed closure",
            json!({
                "sealedLockHash": sealed.content_hash(),
                "computedLockHash": computed.content_hash(),
            }),
        ));
    }
    Ok(LoadedPackage {
        lock_hash: sealed.content_hash(),
        lock: sealed,
        program,
    })
}

pub fn verify_package(
    store: impl AsRef<Path>,
    content_hash: &str,
) -> YanshuResult<PackageManifest> {
    Ok(verify_artifact(&absolute_path(store.as_ref())?, content_hash)?.manifest)
}

impl BuildContext {
    fn build(&mut self, root: &Path) -> YanshuResult<&BuiltPackage> {
        if self.built.contains_key(root) {
            return self.built.get(root).ok_or_else(internal_error);
        }
        if !self.visiting.insert(root.to_path_buf()) {
            return Err(Diagnostic::new(
                "PACKAGE_SOURCE_DEPENDENCY_CYCLE",
                "source package dependencies contain a cycle",
                json!({ "path": root.display().to_string() }),
            ));
        }
        let descriptor = read_source_descriptor(root)?;
        let mut dependencies = Vec::new();
        let mut dependency_modules = BTreeSet::new();
        for dependency in &descriptor.dependencies {
            let dependency_root = canonical_directory(
                &root.join(&dependency.path),
                "PACKAGE_DEPENDENCY_PATH_MISSING",
            )?;
            if !dependency_root.starts_with(&self.workspace_root) {
                return Err(Diagnostic::new(
                    "PACKAGE_DEPENDENCY_PATH_ESCAPE",
                    "source dependency resolves outside the package workspace",
                    json!({ "dependency": dependency.name, "path": dependency.path }),
                ));
            }
            let built = self.build(&dependency_root)?;
            if built.name != dependency.name {
                return Err(Diagnostic::new(
                    "PACKAGE_DEPENDENCY_NAME_MISMATCH",
                    "source dependency name does not match its package descriptor",
                    json!({ "expected": dependency.name, "actual": built.name }),
                ));
            }
            dependency_modules.extend(
                built
                    .manifest
                    .modules
                    .iter()
                    .map(|module| module.name.clone()),
            );
            dependencies.push(PackageDependency {
                name: built.name.clone(),
                content_hash: built.content_hash.clone(),
            });
        }
        dependencies.sort_by(|left, right| left.name.cmp(&right.name));
        let (mut modules, sources, programs, language_version) = read_modules(root, &descriptor)?;
        modules.sort_by(|left, right| left.name.cmp(&right.name));
        let local_modules = programs.keys().cloned().collect::<BTreeSet<_>>();
        if !local_modules.contains(&descriptor.entry) {
            return Err(Diagnostic::new(
                "PACKAGE_ENTRY_MISSING",
                "package entry must name one of its own modules",
                json!({ "entry": descriptor.entry }),
            ));
        }
        for program in programs.values() {
            for import in &program.imports {
                if !local_modules.contains(import) && !dependency_modules.contains(import) {
                    return Err(Diagnostic::new(
                        "PACKAGE_UNDECLARED_MODULE_IMPORT",
                        "module import is not provided locally or by a direct package dependency",
                        json!({ "module": program.name, "import": import }),
                    ));
                }
            }
        }
        let manifest = PackageManifest {
            name: descriptor.name.clone(),
            version: descriptor.version,
            entry: descriptor.entry,
            language_version,
            modules,
            dependencies,
        };
        let content_hash = manifest.content_hash();
        write_artifact(&self.store_root, &content_hash, &manifest, &sources)?;
        self.visiting.remove(root);
        self.built.insert(
            root.to_path_buf(),
            BuiltPackage {
                name: descriptor.name,
                content_hash,
                manifest,
            },
        );
        self.built.get(root).ok_or_else(internal_error)
    }
}

fn read_source_descriptor(root: &Path) -> YanshuResult<SourceDescriptor> {
    let document = read_json(
        &root.join(SOURCE_DESCRIPTOR),
        MAXIMUM_DOCUMENT_BYTES,
        "package source descriptor",
    )?;
    source_descriptor(&document)
}

type ModuleSources = BTreeMap<String, Vec<u8>>;
type Programs = BTreeMap<String, Program>;

fn read_modules(
    root: &Path,
    descriptor: &SourceDescriptor,
) -> YanshuResult<(Vec<PackageModule>, ModuleSources, Programs, u64)> {
    let mut modules = Vec::new();
    let mut sources = BTreeMap::new();
    let mut programs = BTreeMap::new();
    let mut language_version = None;
    for relative in &descriptor.modules {
        let path = root.join(relative);
        let canonical = path.canonicalize().map_err(|error| {
            io_diagnostic(
                "PACKAGE_MODULE_READ",
                "could not resolve a package module",
                &path,
                &error,
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(Diagnostic::new(
                "PACKAGE_MODULE_PATH_ESCAPE",
                "package module resolves outside its source package",
                json!({ "path": relative }),
            ));
        }
        let bytes = read_bounded(&canonical, MAXIMUM_SOURCE_BYTES, "PACKAGE_MODULE_READ")?;
        let source = std::str::from_utf8(&bytes).map_err(|_| {
            Diagnostic::new(
                "PACKAGE_MODULE_UTF8",
                "package module is not valid UTF-8",
                json!({ "path": relative }),
            )
        })?;
        let program = load_program_source(source)?;
        let version = program.version.to_string().parse::<u64>().map_err(|_| {
            Diagnostic::simple(
                "PACKAGE_LANGUAGE_VERSION",
                "package module language version is not representable",
            )
        })?;
        if language_version.is_some_and(|existing| existing != version) {
            return Err(Diagnostic::simple(
                "PACKAGE_LANGUAGE_VERSION_CONFLICT",
                "all modules in a package must use the same language version",
            ));
        }
        language_version = Some(version);
        let name = program.name.clone();
        if programs.insert(name.clone(), program).is_some() {
            return Err(Diagnostic::simple(
                "PACKAGE_DUPLICATE_MODULE",
                "package contains a module name more than once",
            ));
        }
        modules.push(PackageModule {
            name,
            path: relative.clone(),
            sha256: sha256(&bytes),
        });
        sources.insert(relative.clone(), bytes);
    }
    Ok((
        modules,
        sources,
        programs,
        language_version.ok_or_else(|| {
            Diagnostic::simple("PACKAGE_EMPTY", "package must contain at least one module")
        })?,
    ))
}

fn write_artifact(
    store: &Path,
    content_hash: &str,
    manifest: &PackageManifest,
    sources: &BTreeMap<String, Vec<u8>>,
) -> YanshuResult<()> {
    let shard = store.join("sha256");
    let target = shard.join(content_hash);
    if target.exists() {
        let existing = verify_artifact(store, content_hash)?;
        if existing.manifest == *manifest {
            return Ok(());
        }
        return Err(Diagnostic::simple(
            "PACKAGE_STORE_COLLISION",
            "existing package artifact does not match its content hash",
        ));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = shard.join(format!(
        ".tmp-{}-{nonce}-{}",
        std::process::id(),
        &content_hash[..12]
    ));
    fs::create_dir(&temporary).map_err(|error| {
        io_diagnostic(
            "PACKAGE_STORE_CREATE",
            "could not create a temporary package artifact",
            &temporary,
            &error,
        )
    })?;
    let guard = TemporaryArtifact(temporary.clone());
    for (relative, bytes) in sources {
        let destination = temporary.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                io_diagnostic(
                    "PACKAGE_STORE_WRITE",
                    "could not create package module directory",
                    parent,
                    &error,
                )
            })?;
        }
        fs::write(&destination, bytes).map_err(|error| {
            io_diagnostic(
                "PACKAGE_STORE_WRITE",
                "could not write a package module",
                &destination,
                &error,
            )
        })?;
    }
    write_json(
        &temporary.join(ARTIFACT_MANIFEST),
        &manifest.to_json(),
        "PACKAGE_STORE_WRITE",
    )?;
    match fs::rename(&temporary, &target) {
        Ok(()) => {
            guard.keep();
            Ok(())
        }
        Err(error) if target.exists() => {
            let existing = verify_artifact(store, content_hash)?;
            if existing.manifest == *manifest {
                Ok(())
            } else {
                Err(io_diagnostic(
                    "PACKAGE_STORE_COLLISION",
                    "concurrent package artifact does not match",
                    &target,
                    &error,
                ))
            }
        }
        Err(error) => Err(io_diagnostic(
            "PACKAGE_STORE_WRITE",
            "could not publish the package artifact",
            &target,
            &error,
        )),
    }
}

fn verify_artifact(store: &Path, content_hash: &str) -> YanshuResult<VerifiedArtifact> {
    if !valid_hash(content_hash) {
        return Err(Diagnostic::simple(
            "PACKAGE_INVALID_HASH",
            "package hash must be lowercase SHA-256",
        ));
    }
    let root = store.join("sha256").join(content_hash);
    let canonical_shard = canonical_directory(&store.join("sha256"), "PACKAGE_STORE_MISSING")?;
    let canonical_root = canonical_directory(&root, "PACKAGE_ARTIFACT_MISSING")?;
    if !canonical_root.starts_with(&canonical_shard) {
        return Err(Diagnostic::new(
            "PACKAGE_ARTIFACT_PATH_ESCAPE",
            "package artifact resolves outside the content-addressed store",
            json!({ "contentHash": content_hash }),
        ));
    }
    let document = read_json(
        &canonical_root.join(ARTIFACT_MANIFEST),
        MAXIMUM_DOCUMENT_BYTES,
        "package manifest",
    )?;
    let manifest = package_manifest(&document)?;
    if manifest.content_hash() != content_hash {
        return Err(Diagnostic::new(
            "PACKAGE_CONTENT_HASH_MISMATCH",
            "package manifest does not match its content-addressed store path",
            json!({ "expected": content_hash, "actual": manifest.content_hash() }),
        ));
    }
    let mut programs = BTreeMap::new();
    for module in &manifest.modules {
        let path = canonical_root.join(&module.path);
        let canonical = path.canonicalize().map_err(|error| {
            io_diagnostic(
                "PACKAGE_MODULE_READ",
                "could not resolve a stored package module",
                &path,
                &error,
            )
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(Diagnostic::new(
                "PACKAGE_MODULE_PATH_ESCAPE",
                "stored package module resolves outside its artifact",
                json!({ "path": module.path }),
            ));
        }
        let bytes = read_bounded(&canonical, MAXIMUM_SOURCE_BYTES, "PACKAGE_MODULE_READ")?;
        if sha256(&bytes) != module.sha256 {
            return Err(Diagnostic::new(
                "PACKAGE_MODULE_HASH_MISMATCH",
                "stored package module does not match its manifest hash",
                json!({ "module": module.name }),
            ));
        }
        let source = std::str::from_utf8(&bytes).map_err(|_| {
            Diagnostic::simple("PACKAGE_MODULE_UTF8", "stored package module is not UTF-8")
        })?;
        let program = load_program_source(source)?;
        if program.name != module.name
            || program.version.to_string() != manifest.language_version.to_string()
        {
            return Err(Diagnostic::new(
                "PACKAGE_MODULE_IDENTITY_MISMATCH",
                "stored module identity does not match its package manifest",
                json!({ "module": module.name }),
            ));
        }
        programs.insert(program.name.clone(), program);
    }
    Ok(VerifiedArtifact { manifest, programs })
}

fn resolve_store(store: &Path, root_hash: &str) -> YanshuResult<(PackageLock, Program)> {
    let mut artifacts = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    collect_artifact(store, root_hash, &mut visiting, &mut artifacts)?;
    let root = artifacts.get(root_hash).ok_or_else(internal_error)?;
    let mut package_names = BTreeMap::new();
    let mut programs = BTreeMap::new();
    for (content_hash, artifact) in &artifacts {
        if let Some(existing) = package_names.insert(artifact.manifest.name.clone(), content_hash)
            && existing != content_hash
        {
            return Err(Diagnostic::new(
                "PACKAGE_NAME_CONFLICT",
                "dependency closure contains multiple hashes for one package name",
                json!({ "package": artifact.manifest.name }),
            ));
        }
        if artifact.manifest.language_version != root.manifest.language_version {
            return Err(Diagnostic::simple(
                "PACKAGE_LANGUAGE_VERSION_CONFLICT",
                "all packages in a lock closure must use one language version",
            ));
        }
        for (name, program) in &artifact.programs {
            if programs.insert(name.clone(), program.clone()).is_some() {
                return Err(Diagnostic::new(
                    "PACKAGE_MODULE_NAME_CONFLICT",
                    "dependency closure contains the same module name more than once",
                    json!({ "module": name }),
                ));
            }
        }
    }
    for program in programs.values() {
        for import in &program.imports {
            if !programs.contains_key(import) {
                return Err(Diagnostic::new(
                    "PACKAGE_LOCK_IMPORT_MISSING",
                    "locked package closure does not provide an imported module",
                    json!({ "module": program.name, "import": import }),
                ));
            }
        }
    }
    let program = link_program_set(&programs, &root.manifest.entry)?;
    let capability_closure = if root.manifest.language_version >= 4 {
        analyze_program(&program)?.capability_closure
    } else {
        Vec::new()
    };
    let mut packages = artifacts
        .iter()
        .map(|(content_hash, artifact)| LockedPackage {
            name: artifact.manifest.name.clone(),
            version: artifact.manifest.version.clone(),
            content_hash: content_hash.clone(),
            dependencies: artifact.manifest.dependencies.clone(),
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    let lock = PackageLock {
        root_package: root_hash.to_owned(),
        entry_module: root.manifest.entry.clone(),
        language_version: root.manifest.language_version,
        capability_closure,
        packages,
    };
    Ok((lock, program))
}

fn collect_artifact(
    store: &Path,
    content_hash: &str,
    visiting: &mut BTreeSet<String>,
    artifacts: &mut BTreeMap<String, VerifiedArtifact>,
) -> YanshuResult<()> {
    if artifacts.contains_key(content_hash) {
        return Ok(());
    }
    if !visiting.insert(content_hash.to_owned()) {
        return Err(Diagnostic::simple(
            "PACKAGE_DEPENDENCY_CYCLE",
            "content-addressed package dependencies contain a cycle",
        ));
    }
    let artifact = verify_artifact(store, content_hash)?;
    for dependency in &artifact.manifest.dependencies {
        collect_artifact(store, &dependency.content_hash, visiting, artifacts)?;
        let actual = artifacts
            .get(&dependency.content_hash)
            .ok_or_else(internal_error)?;
        if actual.manifest.name != dependency.name {
            return Err(Diagnostic::new(
                "PACKAGE_DEPENDENCY_NAME_MISMATCH",
                "content-addressed dependency name does not match its artifact",
                json!({ "expected": dependency.name, "actual": actual.manifest.name }),
            ));
        }
    }
    visiting.remove(content_hash);
    artifacts.insert(content_hash.to_owned(), artifact);
    Ok(())
}

struct TemporaryArtifact(PathBuf);

impl TemporaryArtifact {
    fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for TemporaryArtifact {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_json(path: &Path, maximum: u64, kind: &str) -> YanshuResult<Value> {
    let bytes = read_bounded(path, maximum, "PACKAGE_DOCUMENT_READ")?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Diagnostic::new(
            "PACKAGE_INVALID_JSON",
            format!("{kind} is not valid JSON"),
            json!({ "line": error.line(), "column": error.column() }),
        )
    })
}

fn write_json(path: &Path, document: &Value, code: &'static str) -> YanshuResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| {
            io_diagnostic(
                code,
                "could not create package document directory",
                parent,
                &error,
            )
        })?;
    }
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|_| Diagnostic::simple(code, "could not encode the package document"))?;
    fs::write(path, bytes)
        .map_err(|error| io_diagnostic(code, "could not write the package document", path, &error))
}

fn read_bounded(path: &Path, maximum: u64, code: &'static str) -> YanshuResult<Vec<u8>> {
    let metadata = fs::metadata(path)
        .map_err(|error| io_diagnostic(code, "could not inspect a package file", path, &error))?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(Diagnostic::new(
            "PACKAGE_FILE_LIMIT",
            "package file is not regular or exceeds its byte limit",
            json!({ "maximum": maximum }),
        ));
    }
    fs::read(path)
        .map_err(|error| io_diagnostic(code, "could not read a package file", path, &error))
}

fn canonical_directory(path: &Path, code: &'static str) -> YanshuResult<PathBuf> {
    let canonical = path.canonicalize().map_err(|error| {
        io_diagnostic(code, "could not resolve a package directory", path, &error)
    })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(Diagnostic::new(
            code,
            "package path is not a directory",
            json!({ "path": path.display().to_string() }),
        ))
    }
}

fn absolute_path(path: &Path) -> YanshuResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| {
                io_diagnostic(
                    "PACKAGE_CURRENT_DIRECTORY",
                    "could not resolve the current directory",
                    path,
                    &error,
                )
            })
    }
}

fn io_diagnostic(
    code: &'static str,
    message: &str,
    path: &Path,
    error: &std::io::Error,
) -> Diagnostic {
    Diagnostic::new(
        code,
        message,
        json!({ "path": path.display().to_string(), "kind": error.kind().to_string() }),
    )
}

fn internal_error() -> Diagnostic {
    Diagnostic::simple(
        "PACKAGE_INTERNAL",
        "package resolver lost an internal verified record",
    )
}
