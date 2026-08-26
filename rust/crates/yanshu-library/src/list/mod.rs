#![forbid(unsafe_code)]

mod measure;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{BackendDescriptor, LibraryBackend, LibraryKey, LibraryValue};
use measure::{
    LimitContext, Metrics, measure_arguments, measure_list, measure_list_iter, measure_ok_list,
    measure_ok_value, measure_value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOperation {
    Reverse,
    Append,
    Contains,
    Get,
    Take,
    Drop,
    Slice,
}

#[derive(Debug, Default)]
pub struct RustListBackend;

impl LibraryBackend for RustListBackend {
    fn descriptor(&self) -> BackendDescriptor {
        BackendDescriptor {
            provider: "rust-std".to_owned(),
            library: "list".to_owned(),
            version: 1,
            operations: vec![
                "reverse".to_owned(),
                "append".to_owned(),
                "contains?".to_owned(),
                "get".to_owned(),
                "take".to_owned(),
                "drop".to_owned(),
                "slice".to_owned(),
            ],
        }
    }

    fn invoke(
        &mut self,
        operation: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<LibraryValue> {
        match operation {
            "reverse" => {
                let values = list_items(&arguments[0]);
                Ok(list_value(values.iter().rev().cloned().collect()))
            }
            "append" => {
                let left = list_items(&arguments[0]);
                let right = list_items(&arguments[1]);
                Ok(list_value(
                    left.iter().chain(right).cloned().collect::<Vec<_>>(),
                ))
            }
            "contains?" => Ok(LibraryValue::Bool(
                list_items(&arguments[0]).contains(&arguments[1]),
            )),
            "get" => Ok(get_result(list_items(&arguments[0]), &arguments[1])),
            "take" => Ok(range_result(
                list_items(&arguments[0]),
                &arguments[1],
                RangeKind::Take,
            )),
            "drop" => Ok(range_result(
                list_items(&arguments[0]),
                &arguments[1],
                RangeKind::Drop,
            )),
            "slice" => Ok(slice_result(
                list_items(&arguments[0]),
                &arguments[1],
                &arguments[2],
            )),
            _ => Err(Diagnostic::simple(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "list backend received an operation outside list@1",
            )),
        }
    }
}

pub(crate) fn list_fuel_work(
    operation: ListOperation,
    arguments: &[LibraryValue],
) -> YanshuResult<u64> {
    let input = measure_arguments(arguments, LimitContext::Argument)?;
    let output = match operation {
        ListOperation::Reverse => measure_list(list_items(&arguments[0]), 0, LimitContext::Result)?,
        ListOperation::Append => {
            let left = list_items(&arguments[0]);
            let right = list_items(&arguments[1]);
            measure_list_iter(
                left.iter().chain(right),
                left.len().saturating_add(right.len()),
                0,
                LimitContext::Result,
            )?
        }
        ListOperation::Contains => Metrics::single_node(),
        ListOperation::Get => {
            let values = list_items(&arguments[0]);
            index(&arguments[1])
                .and_then(|index| values.get(index))
                .map_or_else(
                    || {
                        measure_value(
                            &list_issue("LIST_INDEX_OUT_OF_BOUNDS", values.len()),
                            0,
                            LimitContext::Result,
                        )
                    },
                    |value| measure_ok_value(value, LimitContext::Result),
                )?
        }
        ListOperation::Take => {
            let values = list_items(&arguments[0]);
            let Some(count) = index(&arguments[1]).filter(|count| *count <= values.len()) else {
                return combine_with_issue_work(input, "LIST_COUNT_OUT_OF_BOUNDS", values.len());
            };
            measure_ok_list(&values[..count], LimitContext::Result)?
        }
        ListOperation::Drop => {
            let values = list_items(&arguments[0]);
            let Some(count) = index(&arguments[1]).filter(|count| *count <= values.len()) else {
                return combine_with_issue_work(input, "LIST_COUNT_OUT_OF_BOUNDS", values.len());
            };
            measure_ok_list(&values[count..], LimitContext::Result)?
        }
        ListOperation::Slice => {
            let values = list_items(&arguments[0]);
            let Some((start, end)) = range(&arguments[1], &arguments[2], values.len()) else {
                return combine_with_issue_work(input, "LIST_RANGE_OUT_OF_BOUNDS", values.len());
            };
            measure_ok_list(&values[start..end], LimitContext::Result)?
        }
    };
    Ok(input.work().saturating_add(output.work()))
}

fn combine_with_issue_work(input: Metrics, code: &'static str, length: usize) -> YanshuResult<u64> {
    let output = measure_value(&list_issue(code, length), 0, LimitContext::Result)?;
    Ok(input.work().saturating_add(output.work()))
}

fn get_result(values: &[LibraryValue], raw_index: &LibraryValue) -> LibraryValue {
    index(raw_index)
        .and_then(|index| values.get(index))
        .cloned()
        .map_or_else(
            || list_issue("LIST_INDEX_OUT_OF_BOUNDS", values.len()),
            |value| LibraryValue::Ok(Box::new(value)),
        )
}

#[derive(Debug, Clone, Copy)]
enum RangeKind {
    Take,
    Drop,
}

fn range_result(
    values: &[LibraryValue],
    raw_count: &LibraryValue,
    kind: RangeKind,
) -> LibraryValue {
    let Some(count) = index(raw_count).filter(|count| *count <= values.len()) else {
        return list_issue("LIST_COUNT_OUT_OF_BOUNDS", values.len());
    };
    let selected = match kind {
        RangeKind::Take => &values[..count],
        RangeKind::Drop => &values[count..],
    };
    LibraryValue::Ok(Box::new(list_value(selected.to_vec())))
}

fn slice_result(
    values: &[LibraryValue],
    raw_start: &LibraryValue,
    raw_end: &LibraryValue,
) -> LibraryValue {
    let Some((start, end)) = range(raw_start, raw_end, values.len()) else {
        return list_issue("LIST_RANGE_OUT_OF_BOUNDS", values.len());
    };
    LibraryValue::Ok(Box::new(list_value(values[start..end].to_vec())))
}

fn index(value: &LibraryValue) -> Option<usize> {
    let LibraryValue::Int(value) = value else {
        return None;
    };
    value.to_usize()
}

fn range(start: &LibraryValue, end: &LibraryValue, length: usize) -> Option<(usize, usize)> {
    let start = index(start)?;
    let end = index(end)?;
    (start <= end && end <= length).then_some((start, end))
}

fn list_items(value: &LibraryValue) -> &[LibraryValue] {
    match value {
        LibraryValue::Nil => &[],
        LibraryValue::List(values) => values,
        _ => unreachable!("list@1 arguments are validated by the trusted contract"),
    }
}

fn list_value(values: Vec<LibraryValue>) -> LibraryValue {
    if values.is_empty() {
        LibraryValue::Nil
    } else {
        LibraryValue::List(values)
    }
}

fn list_issue(code: &'static str, length: usize) -> LibraryValue {
    LibraryValue::Err(Box::new(LibraryValue::Map(BTreeMap::from([
        (
            LibraryKey::String("code".to_owned()),
            LibraryValue::String(code.to_owned()),
        ),
        (
            LibraryKey::String("length".to_owned()),
            LibraryValue::Int(BigInt::from(length)),
        ),
    ]))))
}
