#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use yanshu_diagnostic::YanshuResult;

use super::{MergeKind, library_key, map_issue, map_items};
use crate::portable::{
    LimitContext, Metrics, measure_arguments, measure_key_value, measure_map_iter, measure_value,
};
use crate::{LibraryKey, LibraryValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapOperation {
    Size,
    Keys,
    Values,
    Entries,
    ContainsValue,
    Remove,
    MergeDisjoint,
    MergeLeft,
    MergeRight,
}

pub(crate) fn map_fuel_work(
    operation: MapOperation,
    arguments: &[LibraryValue],
) -> YanshuResult<u64> {
    let input = measure_arguments(arguments, LimitContext::Argument)?;
    let mapping = map_items(&arguments[0]);
    let output = match operation {
        MapOperation::Size | MapOperation::ContainsValue => Metrics::single_node(),
        MapOperation::Keys => measure_keys(mapping)?,
        MapOperation::Values => measure_values(mapping)?,
        MapOperation::Entries => measure_entries(mapping)?,
        MapOperation::Remove => measure_remove(mapping, &arguments[1])?,
        MapOperation::MergeDisjoint => {
            measure_merge(mapping, map_items(&arguments[1]), MergeKind::Disjoint)?
        }
        MapOperation::MergeLeft => {
            measure_merge(mapping, map_items(&arguments[1]), MergeKind::Left)?
        }
        MapOperation::MergeRight => {
            measure_merge(mapping, map_items(&arguments[1]), MergeKind::Right)?
        }
    };
    Ok(input.work().saturating_add(output.work()))
}

fn measure_keys(mapping: &BTreeMap<LibraryKey, LibraryValue>) -> YanshuResult<Metrics> {
    let mut metrics = Metrics::single_node();
    for key in mapping.keys() {
        metrics.add(
            measure_key_value(key, 1, LimitContext::Result)?,
            LimitContext::Result,
        )?;
    }
    Ok(metrics)
}

fn measure_values(mapping: &BTreeMap<LibraryKey, LibraryValue>) -> YanshuResult<Metrics> {
    let mut metrics = Metrics::single_node();
    for value in mapping.values() {
        metrics.add(
            measure_value(value, 1, LimitContext::Result)?,
            LimitContext::Result,
        )?;
    }
    Ok(metrics)
}

fn measure_entries(mapping: &BTreeMap<LibraryKey, LibraryValue>) -> YanshuResult<Metrics> {
    let mut metrics = Metrics::single_node();
    for (key, value) in mapping {
        let mut entry = Metrics::single_node();
        entry.add(
            measure_key_value(key, 2, LimitContext::Result)?,
            LimitContext::Result,
        )?;
        entry.add(
            measure_value(value, 2, LimitContext::Result)?,
            LimitContext::Result,
        )?;
        metrics.add(entry, LimitContext::Result)?;
    }
    Ok(metrics)
}

fn measure_remove(
    mapping: &BTreeMap<LibraryKey, LibraryValue>,
    raw_key: &LibraryValue,
) -> YanshuResult<Metrics> {
    let Some(key) = library_key(raw_key) else {
        return measure_value(&map_issue("MAP_INVALID_KEY", None), 0, LimitContext::Result);
    };
    let length = mapping
        .len()
        .saturating_sub(usize::from(mapping.contains_key(&key)));
    let mut metrics = Metrics::single_node();
    metrics.add(
        measure_map_iter(
            mapping.iter().filter(|(candidate, _)| *candidate != &key),
            length,
            1,
            LimitContext::Result,
        )?,
        LimitContext::Result,
    )?;
    Ok(metrics)
}

fn measure_merge(
    left: &BTreeMap<LibraryKey, LibraryValue>,
    right: &BTreeMap<LibraryKey, LibraryValue>,
    kind: MergeKind,
) -> YanshuResult<Metrics> {
    let conflicts = right.keys().filter(|key| left.contains_key(*key)).count();
    if matches!(kind, MergeKind::Disjoint) && conflicts > 0 {
        return measure_value(
            &map_issue("MAP_KEY_CONFLICT", Some(conflicts)),
            0,
            LimitContext::Result,
        );
    }

    let added = right.keys().filter(|key| !left.contains_key(*key)).count();
    let length = left.len().saturating_add(added);
    let map_depth = usize::from(matches!(kind, MergeKind::Disjoint));
    let merged = left
        .iter()
        .map(|(key, left_value)| {
            let value = if matches!(kind, MergeKind::Right) {
                right.get(key).unwrap_or(left_value)
            } else {
                left_value
            };
            (key, value)
        })
        .chain(right.iter().filter(|(key, _)| !left.contains_key(*key)));
    let map = measure_map_iter(merged, length, map_depth, LimitContext::Result)?;
    if matches!(kind, MergeKind::Disjoint) {
        let mut result = Metrics::single_node();
        result.add(map, LimitContext::Result)?;
        Ok(result)
    } else {
        Ok(map)
    }
}
