#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::Program;
use serde_json::json;

pub(crate) fn dependency_order(
    programs: &BTreeMap<String, Program>,
    entry: &str,
) -> AilResult<Vec<String>> {
    if !programs.contains_key(entry) {
        return Err(Diagnostic::new(
            "BUNDLE_ENTRY_MISSING",
            "bundle entry module is missing",
            json!({ "entry": entry }),
        ));
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    visit(entry, programs, &mut visiting, &mut visited, &mut order)?;
    if visited.len() != programs.len() {
        let unreachable = programs
            .keys()
            .filter(|name| !visited.contains(*name))
            .cloned()
            .collect::<Vec<_>>();
        return Err(Diagnostic::new(
            "BUNDLE_UNREACHABLE_MODULE",
            "bundle contains modules unreachable from its entry",
            json!({ "modules": unreachable }),
        ));
    }
    Ok(order)
}

fn visit(
    name: &str,
    programs: &BTreeMap<String, Program>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) -> AilResult<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_owned()) {
        return Err(Diagnostic::new(
            "BUNDLE_IMPORT_CYCLE",
            "bundle module imports contain a cycle",
            json!({ "module": name }),
        ));
    }
    let program = programs.get(name).ok_or_else(|| {
        Diagnostic::new(
            "BUNDLE_IMPORT_MISSING",
            "bundle import does not name a sealed module",
            json!({ "module": name }),
        )
    })?;
    for dependency in &program.imports {
        visit(dependency, programs, visiting, visited, order)?;
    }
    visiting.remove(name);
    visited.insert(name.to_owned());
    order.push(name.to_owned());
    Ok(())
}
