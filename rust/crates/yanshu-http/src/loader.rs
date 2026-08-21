#![forbid(unsafe_code)]

use std::path::Path;

use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_store::VersionStore;
use yanshu_syntax::{Program, load_program_source};

pub trait ProgramLoader: Send + Sync {
    fn load(&self) -> YanshuResult<LoadedProgram>;
}

#[derive(Debug, Clone)]
pub struct LoadedProgram {
    pub program: Program,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FixedProgramLoader {
    program: Program,
}

impl FixedProgramLoader {
    pub fn from_source(source: &str) -> YanshuResult<Self> {
        Ok(Self {
            program: load_program_source(source)?,
        })
    }
}

impl ProgramLoader for FixedProgramLoader {
    fn load(&self) -> YanshuResult<LoadedProgram> {
        Ok(LoadedProgram {
            program: self.program.clone(),
            version: None,
        })
    }
}

#[derive(Debug)]
pub struct ActiveVersionLoader {
    store: VersionStore,
}

impl ActiveVersionLoader {
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            store: VersionStore::new(root),
        }
    }
}

impl ProgramLoader for ActiveVersionLoader {
    fn load(&self) -> YanshuResult<LoadedProgram> {
        let version = self.store.active_hash()?.ok_or_else(|| {
            Diagnostic::simple("VERSION_NO_ACTIVE", "version store has no active version")
        })?;
        let source = self.store.version_source(&version)?;
        Ok(LoadedProgram {
            program: load_program_source(&source)?,
            version: Some(version),
        })
    }
}
