#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use yanshu_diagnostic::YanshuResult;
use yanshu_syntax::{Pattern, PatternKind};

use crate::{
    Budget, Value,
    value::{measure_datum, measure_runtime_value},
};

pub(crate) fn bindings_for_pattern(
    pattern: &Pattern,
    value: &Value,
    budget: &mut Budget,
) -> YanshuResult<Option<BTreeMap<String, Value>>> {
    let mut bindings = BTreeMap::new();
    if matches_pattern(pattern, value, budget, &mut bindings)? {
        Ok(Some(bindings))
    } else {
        Ok(None)
    }
}

fn matches_pattern(
    pattern: &Pattern,
    value: &Value,
    budget: &mut Budget,
    bindings: &mut BTreeMap<String, Value>,
) -> YanshuResult<bool> {
    budget.consume(1)?;
    match &pattern.kind {
        PatternKind::Wildcard => Ok(true),
        PatternKind::Binding(name) => {
            budget.consume(measure_runtime_value(value)?.fuel_cost())?;
            bindings.insert(name.clone(), value.clone());
            Ok(true)
        }
        PatternKind::Literal(datum) => {
            budget.consume(measure_datum(datum)?.fuel_cost())?;
            Ok(&Value::from(datum) == value)
        }
        PatternKind::Variant { name, fields } => {
            let Value::Variant {
                variant,
                fields: values,
                ..
            } = value
            else {
                return Ok(false);
            };
            if variant != name || fields.len() != values.len() {
                return Ok(false);
            }
            for (field, value) in fields.iter().zip(values) {
                if !matches_pattern(field, value, budget, bindings)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}
