#![forbid(unsafe_code)]

mod measure;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use num_bigint::BigInt;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue};

pub use measure::MapOperation;
pub(crate) use measure::map_fuel_work;

#[derive(Debug, Clone, Copy)]
enum MergeKind {
    Disjoint,
    Left,
    Right,
}

#[derive(Debug, Default)]
pub struct RustMapBackend;

impl LibraryBackend for RustMapBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "map".to_owned(),
            version: 1,
            operations: vec![
                "size".to_owned(),
                "keys".to_owned(),
                "values".to_owned(),
                "entries".to_owned(),
                "contains-value?".to_owned(),
                "remove".to_owned(),
                "merge-disjoint".to_owned(),
                "merge-left".to_owned(),
                "merge-right".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        match operation {
            "size" => Ok(LibraryValue::Int(BigInt::from(
                map_items(&arguments[0]).len(),
            ))),
            "keys" => Ok(list_value(
                map_items(&arguments[0]).keys().map(key_value).collect(),
            )),
            "values" => Ok(list_value(
                map_items(&arguments[0]).values().cloned().collect(),
            )),
            "entries" => Ok(list_value(
                map_items(&arguments[0])
                    .iter()
                    .map(|(key, value)| LibraryValue::List(vec![key_value(key), value.clone()]))
                    .collect(),
            )),
            "contains-value?" => Ok(LibraryValue::Bool(
                map_items(&arguments[0])
                    .values()
                    .any(|value| value == &arguments[1]),
            )),
            "remove" => Ok(remove_result(map_items(&arguments[0]), &arguments[1])),
            "merge-disjoint" => Ok(merge_result(
                map_items(&arguments[0]),
                map_items(&arguments[1]),
                MergeKind::Disjoint,
            )),
            "merge-left" => Ok(LibraryValue::Map(merged_map(
                map_items(&arguments[0]),
                map_items(&arguments[1]),
                MergeKind::Left,
            ))),
            "merge-right" => Ok(LibraryValue::Map(merged_map(
                map_items(&arguments[0]),
                map_items(&arguments[1]),
                MergeKind::Right,
            ))),
            _ => Err(Diagnostic::simple(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "map backend received an operation outside map@1",
            )),
        }
    }
}

fn remove_result(
    mapping: &BTreeMap<LibraryKey, LibraryValue>,
    raw_key: &LibraryValue,
) -> LibraryValue {
    let Some(key) = library_key(raw_key) else {
        return map_issue("MAP_INVALID_KEY", None);
    };
    let mut result = mapping.clone();
    result.remove(&key);
    LibraryValue::Ok(Box::new(LibraryValue::Map(result)))
}

fn merge_result(
    left: &BTreeMap<LibraryKey, LibraryValue>,
    right: &BTreeMap<LibraryKey, LibraryValue>,
    kind: MergeKind,
) -> LibraryValue {
    let conflicts = right.keys().filter(|key| left.contains_key(*key)).count();
    if matches!(kind, MergeKind::Disjoint) && conflicts > 0 {
        return map_issue("MAP_KEY_CONFLICT", Some(conflicts));
    }

    LibraryValue::Ok(Box::new(LibraryValue::Map(merged_map(left, right, kind))))
}

fn merged_map(
    left: &BTreeMap<LibraryKey, LibraryValue>,
    right: &BTreeMap<LibraryKey, LibraryValue>,
    kind: MergeKind,
) -> BTreeMap<LibraryKey, LibraryValue> {
    let mut merged = left.clone();
    for (key, value) in right {
        if matches!(kind, MergeKind::Right) || !merged.contains_key(key) {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn map_items(value: &LibraryValue) -> &BTreeMap<LibraryKey, LibraryValue> {
    match value {
        LibraryValue::Map(mapping) => mapping,
        _ => unreachable!("map@1 arguments are validated by the trusted contract"),
    }
}

fn library_key(value: &LibraryValue) -> Option<LibraryKey> {
    match value {
        LibraryValue::String(value) => Some(LibraryKey::String(value.clone())),
        LibraryValue::Symbol(value) => Some(LibraryKey::Symbol(value.clone())),
        _ => None,
    }
}

fn key_value(key: &LibraryKey) -> LibraryValue {
    match key {
        LibraryKey::String(value) => LibraryValue::String(value.clone()),
        LibraryKey::Symbol(value) => LibraryValue::Symbol(value.clone()),
    }
}

fn list_value(values: Vec<LibraryValue>) -> LibraryValue {
    if values.is_empty() {
        LibraryValue::Nil
    } else {
        LibraryValue::List(values)
    }
}

fn map_issue(code: &'static str, conflicts: Option<usize>) -> LibraryValue {
    let mut fields = BTreeMap::from([(
        LibraryKey::String("code".to_owned()),
        LibraryValue::String(code.to_owned()),
    )]);
    if let Some(conflicts) = conflicts {
        fields.insert(
            LibraryKey::String("conflicts".to_owned()),
            LibraryValue::Int(BigInt::from(conflicts)),
        );
    }
    LibraryValue::Err(Box::new(LibraryValue::Map(fields)))
}
