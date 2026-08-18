#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ail_diagnostic::AilResult;
use ail_syntax::{Pattern, PatternKind};

use crate::{Budget, Value};

pub(crate) fn bindings_for_pattern(
    pattern: &Pattern,
    value: &Value,
    budget: &mut Budget,
) -> AilResult<Option<BTreeMap<String, Value>>> {
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
) -> AilResult<bool> {
    budget.consume(1)?;
    match &pattern.kind {
        PatternKind::Wildcard => Ok(true),
        PatternKind::Binding(name) => {
            bindings.insert(name.clone(), value.clone());
            Ok(true)
        }
        PatternKind::Literal(datum) => Ok(&Value::from(datum) == value),
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
