#![forbid(unsafe_code)]

use ail_syntax::Program;
use serde_json::{Value, json};

use crate::format::hash_json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDescriptor {
    pub name: String,
    pub version: String,
    pub entry: String,
    pub modules: Vec<String>,
    pub dependencies: Vec<SourceDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDependency {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub entry: String,
    pub language_version: u64,
    pub modules: Vec<PackageModule>,
    pub dependencies: Vec<PackageDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageModule {
    pub name: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDependency {
    pub name: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLock {
    pub root_package: String,
    pub entry_module: String,
    pub language_version: u64,
    pub capability_closure: Vec<String>,
    pub packages: Vec<LockedPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub content_hash: String,
    pub dependencies: Vec<PackageDependency>,
}

#[derive(Debug, Clone)]
pub struct LoadedPackage {
    pub lock: PackageLock,
    pub lock_hash: String,
    pub program: Program,
}

impl PackageManifest {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "formatVersion": 1,
            "name": self.name,
            "version": self.version,
            "entry": self.entry,
            "languageVersion": self.language_version,
            "modules": self.modules.iter().map(PackageModule::to_json).collect::<Vec<_>>(),
            "dependencies": self.dependencies.iter().map(PackageDependency::to_json).collect::<Vec<_>>(),
        })
    }

    #[must_use]
    pub fn content_hash(&self) -> String {
        hash_json(&self.to_json())
    }
}

impl PackageModule {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "path": self.path, "sha256": self.sha256 })
    }
}

impl PackageDependency {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "contentHash": self.content_hash })
    }
}

impl PackageLock {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "formatVersion": 1,
            "rootPackage": self.root_package,
            "entryModule": self.entry_module,
            "languageVersion": self.language_version,
            "capabilityClosure": self.capability_closure,
            "packages": self.packages.iter().map(LockedPackage::to_json).collect::<Vec<_>>(),
        })
    }

    #[must_use]
    pub fn content_hash(&self) -> String {
        hash_json(&self.to_json())
    }
}

impl LockedPackage {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "version": self.version,
            "contentHash": self.content_hash,
            "dependencies": self.dependencies.iter().map(PackageDependency::to_json).collect::<Vec<_>>(),
        })
    }
}
