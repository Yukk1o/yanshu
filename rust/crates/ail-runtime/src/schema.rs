#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ail_diagnostic::AilResult;
use ail_syntax::{SchemaField, SchemaKind};
use num_bigint::BigInt;

use crate::{Budget, MapKey, Value};

const MAXIMUM_ISSUES: usize = 32;

pub struct SchemaValidation {
    pub value: Value,
    pub issues: Vec<Value>,
}

impl SchemaValidation {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.issues.is_empty()
    }
}

pub fn validate_schema(
    specification: &SchemaKind,
    value: &Value,
    budget: &mut Budget,
) -> AilResult<SchemaValidation> {
    let mut validator = Validator {
        budget,
        issues: Vec::new(),
        truncated: false,
    };
    let normalized = validator.visit(specification, value, &[])?;
    if validator.truncated {
        validator.issues.truncate(MAXIMUM_ISSUES.saturating_sub(1));
        validator.issues.push(issue(
            "",
            "SCHEMA_ISSUES_TRUNCATED",
            "additional validation issues were omitted",
            [],
        ));
    }
    Ok(SchemaValidation {
        value: normalized,
        issues: validator.issues,
    })
}

struct Validator<'budget> {
    budget: &'budget mut Budget,
    issues: Vec<Value>,
    truncated: bool,
}

impl Validator<'_> {
    fn visit(
        &mut self,
        specification: &SchemaKind,
        value: &Value,
        path: &[String],
    ) -> AilResult<Value> {
        self.budget.consume(1)?;
        match specification {
            SchemaKind::Any => Ok(value.clone()),
            SchemaKind::String {
                minimum_length,
                maximum_length,
            } => {
                let Value::String(text) = value else {
                    self.add_type_issue(path, "string", value);
                    return Ok(value.clone());
                };
                let length = BigInt::from(text.chars().count());
                if &length < minimum_length {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MIN_LENGTH",
                        "string is shorter than the minimum length",
                        [
                            ("minimum", Value::Int(minimum_length.clone())),
                            ("actual", Value::Int(length.clone())),
                        ],
                    ));
                }
                if maximum_length
                    .as_ref()
                    .is_some_and(|maximum| &length > maximum)
                {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MAX_LENGTH",
                        "string is longer than the maximum length",
                        [
                            (
                                "maximum",
                                Value::Int(maximum_length.clone().unwrap_or_default()),
                            ),
                            ("actual", Value::Int(length)),
                        ],
                    ));
                }
                Ok(value.clone())
            }
            SchemaKind::Integer { minimum, maximum } => {
                let Value::Int(integer) = value else {
                    self.add_type_issue(path, "integer", value);
                    return Ok(value.clone());
                };
                if minimum.as_ref().is_some_and(|bound| integer < bound) {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MINIMUM",
                        "integer is below the minimum",
                        [
                            ("minimum", Value::Int(minimum.clone().unwrap_or_default())),
                            ("actual", Value::Int(integer.clone())),
                        ],
                    ));
                }
                if maximum.as_ref().is_some_and(|bound| integer > bound) {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MAXIMUM",
                        "integer is above the maximum",
                        [
                            ("maximum", Value::Int(maximum.clone().unwrap_or_default())),
                            ("actual", Value::Int(integer.clone())),
                        ],
                    ));
                }
                Ok(value.clone())
            }
            SchemaKind::Boolean => {
                if !matches!(value, Value::Bool(_)) {
                    self.add_type_issue(path, "boolean", value);
                }
                Ok(value.clone())
            }
            SchemaKind::List {
                item,
                minimum_length,
                maximum_length,
            } => {
                let Value::List(values) = value else {
                    self.add_type_issue(path, "list", value);
                    return Ok(value.clone());
                };
                let length = u64::try_from(values.len()).unwrap_or(u64::MAX);
                if length < *minimum_length {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MIN_LENGTH",
                        "list is shorter than the minimum length",
                        [
                            ("minimum", Value::Int((*minimum_length).into())),
                            ("actual", Value::Int(length.into())),
                        ],
                    ));
                }
                if length > *maximum_length {
                    self.add_issue(issue(
                        &pointer(path),
                        "SCHEMA_MAX_LENGTH",
                        "list is longer than the maximum length",
                        [
                            ("maximum", Value::Int((*maximum_length).into())),
                            ("actual", Value::Int(length.into())),
                        ],
                    ));
                }
                let mut normalized = Vec::with_capacity(values.len());
                for (index, item_value) in values.iter().enumerate() {
                    let mut child_path = path.to_vec();
                    child_path.push(index.to_string());
                    normalized.push(self.visit(item, item_value, &child_path)?);
                }
                Ok(Value::List(normalized))
            }
            SchemaKind::Object { fields } => self.visit_object(fields, value, path),
        }
    }

    fn visit_object(
        &mut self,
        fields: &[SchemaField],
        value: &Value,
        path: &[String],
    ) -> AilResult<Value> {
        let Value::Map(mapping) = value else {
            self.add_type_issue(path, "object", value);
            return Ok(value.clone());
        };
        let mut normalized = BTreeMap::new();
        for field in fields {
            let key = MapKey::String(field.name.clone());
            if let Some(field_value) = mapping.get(&key) {
                let mut child_path = path.to_vec();
                child_path.push(field.name.clone());
                normalized.insert(
                    key,
                    self.visit(&field.specification, field_value, &child_path)?,
                );
            } else if let Some(default) = &field.default {
                normalized.insert(key, Value::from(default));
            } else if field.required {
                let mut child_path = path.to_vec();
                child_path.push(field.name.clone());
                self.add_issue(issue(
                    &pointer(&child_path),
                    "SCHEMA_REQUIRED",
                    "required field is missing",
                    [],
                ));
            }
        }

        let declared = fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>();
        for key in mapping.keys() {
            if !matches!(key, MapKey::String(name) if declared.contains(&name.as_str())) {
                let mut child_path = path.to_vec();
                child_path.push(key.json_name().to_owned());
                self.add_issue(issue(
                    &pointer(&child_path),
                    "SCHEMA_ADDITIONAL_PROPERTY",
                    "field is not declared by the schema",
                    [],
                ));
            }
        }
        Ok(Value::Map(normalized))
    }

    fn add_type_issue(&mut self, path: &[String], expected: &'static str, value: &Value) {
        self.add_issue(issue(
            &pointer(path),
            "SCHEMA_TYPE",
            &format!("expected a {expected}"),
            [
                ("expected", Value::String(expected.to_owned())),
                ("actual", Value::String(schema_value_kind(value).to_owned())),
            ],
        ));
    }

    fn add_issue(&mut self, value: Value) {
        if self.issues.len() < MAXIMUM_ISSUES {
            self.issues.push(value);
        } else {
            self.truncated = true;
        }
    }
}

fn issue<const N: usize>(
    path: &str,
    code: &str,
    message: &str,
    details: [(&str, Value); N],
) -> Value {
    let mut mapping = BTreeMap::from([
        (
            MapKey::String("path".to_owned()),
            Value::String(path.to_owned()),
        ),
        (
            MapKey::String("code".to_owned()),
            Value::String(code.to_owned()),
        ),
        (
            MapKey::String("message".to_owned()),
            Value::String(message.to_owned()),
        ),
    ]);
    for (key, value) in details {
        mapping.insert(MapKey::String(key.to_owned()), value);
    }
    Value::Map(mapping)
}

fn pointer(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!(
            "/{}",
            path.iter()
                .map(|item| item.replace('~', "~0").replace('/', "~1"))
                .collect::<Vec<_>>()
                .join("/")
        )
    }
}

fn schema_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Nil => "null-or-empty-list",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::Map(_) => "object",
        _ => "unsupported",
    }
}
