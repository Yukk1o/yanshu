#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::Value;

use crate::{
    format::{
        array, exact_fields, object, string, u64_field, valid_hash, valid_name,
        valid_relative_path, valid_version,
    },
    model::{
        LockedPackage, PackageDependency, PackageLock, PackageManifest, PackageModule,
        SourceDependency, SourceDescriptor,
    },
};

const MAXIMUM_PACKAGES: usize = 256;
const MAXIMUM_MODULES: usize = 256;

pub(crate) fn source_descriptor(document: &Value) -> AilResult<SourceDescriptor> {
    let root = object(document, "package source descriptor")?;
    exact_fields(
        root,
        &[
            "formatVersion",
            "name",
            "version",
            "entry",
            "modules",
            "dependencies",
        ],
        "package source descriptor",
    )?;
    if u64_field(root, "formatVersion")? != 1 {
        return Err(Diagnostic::simple(
            "PACKAGE_FORMAT_UNSUPPORTED",
            "package source format is unsupported",
        ));
    }
    let name = checked_name(string(root, "name")?)?;
    let version = checked_version(string(root, "version")?)?;
    let entry = string(root, "entry")?.to_owned();
    let modules = string_list(array(root, "modules")?, true, MAXIMUM_MODULES)?;
    let mut dependencies = Vec::new();
    let mut dependency_names = BTreeSet::new();
    for value in bounded(array(root, "dependencies")?, MAXIMUM_PACKAGES)? {
        let item = object(value, "source dependency")?;
        exact_fields(item, &["name", "path"], "source dependency")?;
        let dependency = SourceDependency {
            name: checked_name(string(item, "name")?)?,
            path: checked_path(string(item, "path")?, false)?,
        };
        if !dependency_names.insert(dependency.name.clone()) {
            return Err(Diagnostic::simple(
                "PACKAGE_DUPLICATE_DEPENDENCY",
                "package declares a dependency more than once",
            ));
        }
        dependencies.push(dependency);
    }
    dependencies.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(SourceDescriptor {
        name,
        version,
        entry,
        modules,
        dependencies,
    })
}

pub(crate) fn package_manifest(document: &Value) -> AilResult<PackageManifest> {
    let root = object(document, "package manifest")?;
    exact_fields(
        root,
        &[
            "formatVersion",
            "name",
            "version",
            "entry",
            "languageVersion",
            "modules",
            "dependencies",
        ],
        "package manifest",
    )?;
    if u64_field(root, "formatVersion")? != 1 {
        return Err(Diagnostic::simple(
            "PACKAGE_FORMAT_UNSUPPORTED",
            "package manifest format is unsupported",
        ));
    }
    let mut modules = Vec::new();
    let mut module_names = BTreeSet::new();
    for value in bounded(array(root, "modules")?, MAXIMUM_MODULES)? {
        let item = object(value, "package module")?;
        exact_fields(item, &["name", "path", "sha256"], "package module")?;
        let module = PackageModule {
            name: string(item, "name")?.to_owned(),
            path: checked_path(string(item, "path")?, true)?,
            sha256: checked_hash(string(item, "sha256")?)?,
        };
        if !module_names.insert(module.name.clone()) {
            return Err(Diagnostic::simple(
                "PACKAGE_DUPLICATE_MODULE",
                "package contains a module name more than once",
            ));
        }
        modules.push(module);
    }
    if modules.is_empty() || !strictly_sorted(modules.iter().map(|item| item.name.as_str())) {
        return Err(Diagnostic::simple(
            "PACKAGE_INVALID_MODULE_ORDER",
            "package modules must be non-empty and sorted by name",
        ));
    }
    let dependencies = package_dependencies(array(root, "dependencies")?)?;
    let entry = string(root, "entry")?.to_owned();
    if !module_names.contains(&entry) {
        return Err(Diagnostic::simple(
            "PACKAGE_ENTRY_MISSING",
            "package entry must name one of its own modules",
        ));
    }
    Ok(PackageManifest {
        name: checked_name(string(root, "name")?)?,
        version: checked_version(string(root, "version")?)?,
        entry,
        language_version: u64_field(root, "languageVersion")?,
        modules,
        dependencies,
    })
}

pub(crate) fn package_lock(document: &Value) -> AilResult<PackageLock> {
    let root = object(document, "package lock")?;
    exact_fields(
        root,
        &[
            "formatVersion",
            "rootPackage",
            "entryModule",
            "languageVersion",
            "capabilityClosure",
            "packages",
        ],
        "package lock",
    )?;
    if u64_field(root, "formatVersion")? != 1 {
        return Err(Diagnostic::simple(
            "PACKAGE_LOCK_FORMAT_UNSUPPORTED",
            "package lock format is unsupported",
        ));
    }
    let root_package = checked_hash(string(root, "rootPackage")?)?;
    let capabilities = string_list(array(root, "capabilityClosure")?, false, 32)?;
    if !strictly_sorted(capabilities.iter().map(String::as_str))
        || capabilities
            .iter()
            .any(|item| !matches!(item.as_str(), "clock" | "kv" | "log"))
    {
        return Err(Diagnostic::simple(
            "PACKAGE_LOCK_INVALID_CAPABILITIES",
            "locked capability closure must be uniquely sorted and supported",
        ));
    }
    let mut packages = Vec::new();
    for value in bounded(array(root, "packages")?, MAXIMUM_PACKAGES)? {
        let item = object(value, "locked package")?;
        exact_fields(
            item,
            &["name", "version", "contentHash", "dependencies"],
            "locked package",
        )?;
        packages.push(LockedPackage {
            name: checked_name(string(item, "name")?)?,
            version: checked_version(string(item, "version")?)?,
            content_hash: checked_hash(string(item, "contentHash")?)?,
            dependencies: package_dependencies(array(item, "dependencies")?)?,
        });
    }
    if packages.is_empty() || !strictly_sorted(packages.iter().map(|item| item.name.as_str())) {
        return Err(Diagnostic::simple(
            "PACKAGE_LOCK_INVALID_ORDER",
            "locked packages must be non-empty and sorted by name",
        ));
    }
    if !packages
        .iter()
        .any(|item| item.content_hash == root_package)
    {
        return Err(Diagnostic::simple(
            "PACKAGE_LOCK_ROOT_MISSING",
            "locked root package is absent from the package list",
        ));
    }
    Ok(PackageLock {
        root_package,
        entry_module: string(root, "entryModule")?.to_owned(),
        language_version: u64_field(root, "languageVersion")?,
        capability_closure: capabilities,
        packages,
    })
}

fn package_dependencies(values: &[Value]) -> AilResult<Vec<PackageDependency>> {
    let mut dependencies = Vec::new();
    for value in bounded(values, MAXIMUM_PACKAGES)? {
        let item = object(value, "package dependency")?;
        exact_fields(item, &["name", "contentHash"], "package dependency")?;
        dependencies.push(PackageDependency {
            name: checked_name(string(item, "name")?)?,
            content_hash: checked_hash(string(item, "contentHash")?)?,
        });
    }
    if !strictly_sorted(dependencies.iter().map(|item| item.name.as_str())) {
        return Err(Diagnostic::simple(
            "PACKAGE_INVALID_DEPENDENCY_ORDER",
            "package dependencies must be uniquely sorted by name",
        ));
    }
    Ok(dependencies)
}

fn string_list(values: &[Value], paths: bool, maximum: usize) -> AilResult<Vec<String>> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for value in bounded(values, maximum)? {
        let item = value.as_str().ok_or_else(|| {
            Diagnostic::simple("PACKAGE_INVALID_DOCUMENT", "array item must be a string")
        })?;
        let item = if paths {
            checked_path(item, true)?
        } else {
            item.to_owned()
        };
        if !seen.insert(item.clone()) {
            return Err(Diagnostic::simple(
                "PACKAGE_DUPLICATE_ITEM",
                "package array contains a duplicate item",
            ));
        }
        result.push(item);
    }
    Ok(result)
}

fn bounded(values: &[Value], maximum: usize) -> AilResult<&[Value]> {
    if values.len() > maximum {
        Err(Diagnostic::simple(
            "PACKAGE_LIMIT_EXCEEDED",
            "package document exceeds its item limit",
        ))
    } else {
        Ok(values)
    }
}

fn checked_name(value: &str) -> AilResult<String> {
    if valid_name(value) {
        Ok(value.to_owned())
    } else {
        Err(Diagnostic::simple(
            "PACKAGE_INVALID_NAME",
            "package name must be a bounded lowercase identifier",
        ))
    }
}

fn checked_version(value: &str) -> AilResult<String> {
    if valid_version(value) {
        Ok(value.to_owned())
    } else {
        Err(Diagnostic::simple(
            "PACKAGE_INVALID_VERSION",
            "package version must be canonical major.minor.patch",
        ))
    }
}

fn checked_hash(value: &str) -> AilResult<String> {
    if valid_hash(value) {
        Ok(value.to_owned())
    } else {
        Err(Diagnostic::simple(
            "PACKAGE_INVALID_HASH",
            "package hash must be lowercase SHA-256",
        ))
    }
}

fn checked_path(value: &str, ail_only: bool) -> AilResult<String> {
    if valid_relative_path(value, ail_only) {
        Ok(value.to_owned())
    } else {
        Err(Diagnostic::simple(
            "PACKAGE_INVALID_PATH",
            "package path must be canonical and relative",
        ))
    }
}

fn strictly_sorted<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let mut previous: Option<&str> = None;
    for value in values {
        if previous.is_some_and(|item| item >= value) {
            return false;
        }
        previous = Some(value);
    }
    true
}
