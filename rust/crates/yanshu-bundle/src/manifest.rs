#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use yanshu_analysis::analyze_program;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::{Program, load_program_source};

use crate::{graph::dependency_order, linker::link_programs};

const MANIFEST_FILE: &str = "bundle.json";
const MAXIMUM_FORMAT_VERSION: u64 = 2;
const MAXIMUM_MODULES: usize = 256;
pub(crate) const MAXIMUM_MANIFEST_BYTES: u64 = 1024 * 1024;
pub(crate) const MAXIMUM_MODULE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleManifest {
    pub name: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleManifest {
    pub format_version: u64,
    pub language_version: u64,
    pub entry: String,
    pub modules: Vec<ModuleManifest>,
    pub capability_closure: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBundle {
    pub manifest: BundleManifest,
    pub bundle_hash: String,
    pub program: Program,
}

impl BundleManifest {
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut document = json!({
            "formatVersion": self.format_version,
            "languageVersion": self.language_version,
            "entry": self.entry,
            "modules": self.modules.iter().map(ModuleManifest::to_json).collect::<Vec<_>>(),
        });
        if let Some(capabilities) = &self.capability_closure {
            document["capabilityClosure"] = json!(capabilities);
        }
        document
    }

    fn canonical_descriptor(&self) -> String {
        self.to_json().to_string()
    }

    #[must_use]
    pub fn content_hash(&self) -> String {
        sha256(self.canonical_descriptor().as_bytes())
    }
}

impl ModuleManifest {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "path": self.path, "sha256": self.sha256 })
    }
}

pub fn seal_bundle_directory(
    root: impl AsRef<Path>,
    entry: &str,
    module_paths: &[String],
) -> YanshuResult<BundleManifest> {
    let root = root.as_ref();
    if module_paths.is_empty() || module_paths.len() > MAXIMUM_MODULES {
        return Err(Diagnostic::new(
            "BUNDLE_INVALID_MODULE_COUNT",
            "bundle must contain between 1 and 256 modules",
            json!({ "actual": module_paths.len(), "maximum": MAXIMUM_MODULES }),
        ));
    }
    let mut modules = Vec::with_capacity(module_paths.len());
    let mut language_version = None;
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for relative in module_paths {
        validate_relative_path(relative)?;
        if !paths.insert(relative.clone()) {
            return Err(bundle_duplicate("path", relative));
        }
        let source = read_module(root, relative)?;
        let program = load_program_source(&source)?;
        if !names.insert(program.name.clone()) {
            return Err(bundle_duplicate("module", &program.name));
        }
        let version = program.version.to_string().parse::<u64>().map_err(|_| {
            Diagnostic::simple(
                "BUNDLE_INVALID_LANGUAGE_VERSION",
                "module language version cannot be represented by the bundle manifest",
            )
        })?;
        if language_version.is_some_and(|expected| expected != version) {
            return Err(Diagnostic::new(
                "BUNDLE_LANGUAGE_VERSION_MISMATCH",
                "all sealed modules must use the same language version",
                json!({ "module": program.name, "actual": version, "expected": language_version }),
            ));
        }
        language_version = Some(version);
        modules.push(ModuleManifest {
            name: program.name,
            path: relative.clone(),
            sha256: sha256(source.as_bytes()),
        });
    }
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    let mut manifest = BundleManifest {
        format_version: if language_version == Some(4) { 2 } else { 1 },
        language_version: language_version.unwrap_or_default(),
        entry: entry.to_owned(),
        modules,
        capability_closure: None,
    };
    let programs = read_verified_programs(root, &manifest)?;
    let order = dependency_order(&programs, entry)?;
    if manifest.format_version == 2 {
        let linked = link_programs(&programs, &order, entry)?;
        manifest.capability_closure = Some(analyze_program(&linked)?.capability_closure);
    }
    let document = serde_json::to_string_pretty(&manifest.to_json()).map_err(|error| {
        Diagnostic::new(
            "BUNDLE_MANIFEST_ENCODE",
            "bundle manifest could not be encoded",
            json!({ "error": error.to_string() }),
        )
    })?;
    fs::write(root.join(MANIFEST_FILE), format!("{document}\n")).map_err(|error| {
        Diagnostic::new(
            "BUNDLE_MANIFEST_WRITE",
            "bundle manifest could not be written",
            json!({ "kind": error.kind().to_string() }),
        )
    })?;
    Ok(manifest)
}

pub fn load_bundle(root: impl AsRef<Path>) -> YanshuResult<LoadedBundle> {
    let root = root.as_ref();
    let bytes = read_bounded(
        &root.join(MANIFEST_FILE),
        MAXIMUM_MANIFEST_BYTES,
        "BUNDLE_MANIFEST_READ",
        "BUNDLE_MANIFEST_LIMIT",
        "bundle manifest",
    )?;
    let manifest = parse_bundle_manifest_bytes(&bytes)?;
    let programs = read_verified_programs(root, &manifest)?;
    let order = dependency_order(&programs, &manifest.entry)?;
    let bundle_hash = manifest.content_hash();
    let mut program = link_programs(&programs, &order, &manifest.entry)?;
    if manifest.format_version == 2 {
        let computed = analyze_program(&program)?.capability_closure;
        if manifest.capability_closure.as_ref() != Some(&computed) {
            return Err(Diagnostic::new(
                "BUNDLE_CAPABILITY_CLOSURE_MISMATCH",
                "sealed capability closure does not match static analysis",
                json!({
                    "sealed": manifest.capability_closure,
                    "computed": computed,
                }),
            ));
        }
    }
    program.source = format!("sealed-bundle:{bundle_hash}");
    Ok(LoadedBundle {
        manifest,
        bundle_hash,
        program,
    })
}

/// Parses an untrusted bundle manifest after enforcing the same byte limit as
/// the filesystem loader.
pub fn parse_bundle_manifest_bytes(bytes: &[u8]) -> YanshuResult<BundleManifest> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_MANIFEST_BYTES {
        return Err(file_limit(
            "BUNDLE_MANIFEST_LIMIT",
            "bundle manifest",
            MAXIMUM_MANIFEST_BYTES,
        ));
    }
    let document: Value = serde_json::from_slice(bytes).map_err(|error| {
        Diagnostic::new(
            "BUNDLE_MANIFEST_JSON",
            "bundle manifest is not valid JSON",
            json!({ "line": error.line(), "column": error.column() }),
        )
    })?;
    parse_manifest(&document)
}

fn read_verified_programs(
    root: &Path,
    manifest: &BundleManifest,
) -> YanshuResult<BTreeMap<String, Program>> {
    let mut programs = BTreeMap::new();
    for module in &manifest.modules {
        validate_relative_path(&module.path)?;
        let source = read_module(root, &module.path)?;
        let actual_hash = sha256(source.as_bytes());
        if actual_hash != module.sha256 {
            return Err(Diagnostic::new(
                "BUNDLE_MODULE_HASH_MISMATCH",
                "sealed module content does not match its manifest hash",
                json!({ "module": module.name, "expected": module.sha256, "actual": actual_hash }),
            ));
        }
        let program = load_program_source(&source)?;
        if program.name != module.name {
            return Err(Diagnostic::new(
                "BUNDLE_MODULE_NAME_MISMATCH",
                "module program name does not match its manifest name",
                json!({ "expected": module.name, "actual": program.name }),
            ));
        }
        if program.version.to_string() != manifest.language_version.to_string() {
            return Err(Diagnostic::new(
                "BUNDLE_LANGUAGE_VERSION_MISMATCH",
                "module language version does not match its manifest",
                json!({ "module": module.name, "expected": manifest.language_version, "actual": program.version.to_string() }),
            ));
        }
        if programs.insert(module.name.clone(), program).is_some() {
            return Err(bundle_duplicate("module", &module.name));
        }
    }
    Ok(programs)
}

fn parse_manifest(document: &Value) -> YanshuResult<BundleManifest> {
    let object = document.as_object().ok_or_else(invalid_manifest)?;
    let format_version = required_u64(object, "formatVersion")?;
    if !(1..=MAXIMUM_FORMAT_VERSION).contains(&format_version) {
        return Err(Diagnostic::new(
            "BUNDLE_FORMAT_UNSUPPORTED",
            "bundle manifest format version is unsupported",
            json!({ "actual": format_version, "minimum": 1, "maximum": MAXIMUM_FORMAT_VERSION }),
        ));
    }
    if format_version == 1 {
        require_exact_fields(
            object,
            &["formatVersion", "languageVersion", "entry", "modules"],
        )?;
    } else {
        require_exact_fields(
            object,
            &[
                "formatVersion",
                "languageVersion",
                "entry",
                "modules",
                "capabilityClosure",
            ],
        )?;
    }
    let language_version = required_u64(object, "languageVersion")?;
    let entry = required_string(object, "entry")?.to_owned();
    let raw_modules = object
        .get("modules")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= MAXIMUM_MODULES)
        .ok_or_else(invalid_manifest)?;
    let mut modules = Vec::with_capacity(raw_modules.len());
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut previous_name: Option<&str> = None;
    for raw_module in raw_modules {
        let module = raw_module.as_object().ok_or_else(invalid_manifest)?;
        require_exact_fields(module, &["name", "path", "sha256"])?;
        let name = required_string(module, "name")?;
        let path = required_string(module, "path")?;
        let hash = required_string(module, "sha256")?;
        validate_relative_path(path)?;
        if hash.len() != 64 || !hash.bytes().all(|value| value.is_ascii_hexdigit()) {
            return Err(Diagnostic::new(
                "BUNDLE_INVALID_HASH",
                "module hash must be 64 hexadecimal characters",
                json!({ "module": name }),
            ));
        }
        if previous_name.is_some_and(|previous| previous >= name) {
            return Err(Diagnostic::simple(
                "BUNDLE_MODULE_ORDER",
                "bundle modules must be uniquely sorted by name",
            ));
        }
        previous_name = Some(name);
        if !names.insert(name.to_owned()) {
            return Err(bundle_duplicate("module", name));
        }
        if !paths.insert(path.to_owned()) {
            return Err(bundle_duplicate("path", path));
        }
        modules.push(ModuleManifest {
            name: name.to_owned(),
            path: path.to_owned(),
            sha256: hash.to_ascii_lowercase(),
        });
    }
    let capability_closure = if format_version == 2 {
        let raw = object
            .get("capabilityClosure")
            .and_then(Value::as_array)
            .ok_or_else(invalid_manifest)?;
        let mut values = Vec::with_capacity(raw.len());
        let mut previous: Option<&str> = None;
        for item in raw {
            let capability = item.as_str().ok_or_else(invalid_manifest)?;
            if !matches!(capability, "clock" | "kv" | "log")
                || previous.is_some_and(|value| value >= capability)
            {
                return Err(Diagnostic::simple(
                    "BUNDLE_INVALID_CAPABILITY_CLOSURE",
                    "capability closure must be uniquely sorted and contain supported capabilities",
                ));
            }
            previous = Some(capability);
            values.push(capability.to_owned());
        }
        Some(values)
    } else {
        None
    };
    if (format_version == 1 && language_version >= 4)
        || (format_version == 2 && language_version != 4)
    {
        return Err(Diagnostic::new(
            "BUNDLE_FORMAT_LANGUAGE_MISMATCH",
            "bundle format does not match its language version",
            json!({ "formatVersion": format_version, "languageVersion": language_version }),
        ));
    }
    Ok(BundleManifest {
        format_version,
        language_version,
        entry,
        modules,
        capability_closure,
    })
}

fn validate_relative_path(value: &str) -> YanshuResult<()> {
    let path = Path::new(value);
    let valid = !value.is_empty()
        && value.len() <= 240
        && value.ends_with(".yan")
        && !value.contains('\\')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid {
        return Err(Diagnostic::new(
            "BUNDLE_INVALID_MODULE_PATH",
            "module path must be a normalized relative .yan path",
            json!({ "path": value }),
        ));
    }
    Ok(())
}

fn read_module(root: &Path, relative: &str) -> YanshuResult<String> {
    let target = root.join(relative);
    ensure_contained(root, &target)?;
    let bytes = read_bounded(
        &target,
        MAXIMUM_MODULE_BYTES,
        "BUNDLE_MODULE_READ",
        "BUNDLE_MODULE_LIMIT",
        "bundle module",
    )?;
    String::from_utf8(bytes).map_err(|_| {
        Diagnostic::new(
            "BUNDLE_MODULE_UTF8",
            "bundle module is not valid UTF-8",
            json!({ "path": relative }),
        )
    })
}

fn read_bounded(
    path: &Path,
    maximum: u64,
    read_code: &'static str,
    limit_code: &'static str,
    kind: &'static str,
) -> YanshuResult<Vec<u8>> {
    let file = File::open(path).map_err(|error| {
        Diagnostic::new(
            read_code,
            format!("{kind} could not be opened"),
            json!({ "kind": error.kind().to_string() }),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        Diagnostic::new(
            read_code,
            format!("{kind} could not be inspected"),
            json!({ "kind": error.kind().to_string() }),
        )
    })?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(file_limit(limit_code, kind, maximum));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len().min(maximum)).unwrap_or(0));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Diagnostic::new(
                read_code,
                format!("{kind} could not be read"),
                json!({ "kind": error.kind().to_string() }),
            )
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(file_limit(limit_code, kind, maximum));
    }
    Ok(bytes)
}

fn file_limit(code: &'static str, kind: &str, maximum: u64) -> Diagnostic {
    Diagnostic::new(
        code,
        format!("{kind} is not a regular file within its byte limit"),
        json!({ "maximum": maximum }),
    )
}

fn ensure_contained(root: &Path, target: &Path) -> YanshuResult<()> {
    let root = fs::canonicalize(root).map_err(|error| path_error(root, error))?;
    let target = fs::canonicalize(target).map_err(|error| path_error(target, error))?;
    if !target.starts_with(&root) {
        return Err(Diagnostic::simple(
            "BUNDLE_PATH_ESCAPE",
            "module path resolves outside the bundle directory",
        ));
    }
    Ok(())
}

fn path_error(path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(
        "BUNDLE_PATH_RESOLUTION",
        "bundle path could not be resolved",
        json!({ "path": path.display().to_string(), "kind": error.kind().to_string() }),
    )
}

fn required_u64(object: &Map<String, Value>, name: &str) -> YanshuResult<u64> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(invalid_manifest)
}

fn required_string<'value>(
    object: &'value Map<String, Value>,
    name: &str,
) -> YanshuResult<&'value str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(invalid_manifest)
}

fn require_exact_fields(object: &Map<String, Value>, fields: &[&str]) -> YanshuResult<()> {
    if object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field)) {
        Ok(())
    } else {
        Err(invalid_manifest())
    }
}

fn invalid_manifest() -> Diagnostic {
    Diagnostic::simple(
        "BUNDLE_INVALID_MANIFEST",
        "bundle manifest has an invalid or unexpected shape",
    )
}

fn bundle_duplicate(kind: &str, value: &str) -> Diagnostic {
    Diagnostic::new(
        "BUNDLE_DUPLICATE_ENTRY",
        "bundle manifest entry is not unique",
        json!({ "kind": kind, "value": value }),
    )
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
