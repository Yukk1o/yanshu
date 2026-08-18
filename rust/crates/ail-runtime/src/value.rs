#![forbid(unsafe_code)]

use std::{collections::BTreeMap, str::FromStr};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::{Datum, DatumKind, SchemaKind};
use num_bigint::BigInt;
use serde_json::{Map, Value as JsonValue, json};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKey {
    String(String),
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(BigInt),
    String(String),
    Symbol(String),
    List(Vec<Self>),
    Map(BTreeMap<MapKey, Self>),
    Ok(Box<Self>),
    Err(Box<Self>),
    Schema {
        name: String,
        specification: SchemaKind,
    },
    Closure(usize),
    Primitive(Primitive),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Primitive {
    pub name: &'static str,
    pub minimum_arity: usize,
    pub maximum_arity: Option<usize>,
    pub operation: PrimitiveOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveOperation {
    Add,
    Multiply,
    Subtract,
    Quotient,
    Remainder,
    Equal,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Not,
    IsInteger,
    IsBoolean,
    IsString,
    IsList,
    IsMap,
    StringAppend,
    List,
    IsEmpty,
    Length,
    First,
    Rest,
    Map,
    Get,
    GetOr,
    HasKey,
    Assoc,
    Validate,
    Ok,
    Err,
    IsOk,
    IsErr,
    ResultValue,
    Unwrap,
    ApiResponse,
    ApiError,
    Log,
    NowMilliseconds,
    KvGet,
    KvPut,
    KvDelete,
    KvList,
    TextLength,
    TextStartsWith,
    TextEndsWith,
    TextContains,
    TextReplace,
}

impl MapKey {
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::String(value) => format!("{value:?}"),
            Self::Symbol(value) => value.clone(),
        }
    }

    #[must_use]
    pub fn json_name(&self) -> &str {
        match self {
            Self::String(value) | Self::Symbol(value) => value,
        }
    }
}

impl Value {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Nil => "Nil",
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::String(_) => "String",
            Self::Symbol(_) => "Symbol",
            Self::List(_) => "List",
            Self::Map(_) => "Map",
            Self::Ok(_) => "Ok",
            Self::Err(_) => "Err",
            Self::Schema { .. } => "Schema",
            Self::Closure(_) => "Function",
            Self::Primitive(_) => "Primitive",
        }
    }

    #[must_use]
    pub fn truthy(&self) -> bool {
        !matches!(self, Self::Bool(false))
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Nil => "()".to_owned(),
            Self::Bool(true) => "#t".to_owned(),
            Self::Bool(false) => "#f".to_owned(),
            Self::Int(value) => value.to_string(),
            Self::String(value) => format!("{value:?}"),
            Self::Symbol(value) => value.clone(),
            Self::List(values) => format!(
                "({})",
                values
                    .iter()
                    .map(Self::display)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Self::Map(_) => "#hash(...)".to_owned(),
            Self::Ok(value) => format!("#(ok {})", value.display()),
            Self::Err(value) => format!("#(err {})", value.display()),
            Self::Schema { name, .. } => format!("#<schema:{name}>"),
            Self::Closure(_) => "#<function>".to_owned(),
            Self::Primitive(primitive) => format!("#<primitive:{}>", primitive.name),
        }
    }

    pub fn to_json(&self) -> AilResult<JsonValue> {
        match self {
            Self::Nil => Ok(JsonValue::Array(Vec::new())),
            Self::Bool(value) => Ok(JsonValue::Bool(*value)),
            Self::Int(value) => Ok(bigint_json(value)),
            Self::String(value) => Ok(JsonValue::String(value.clone())),
            Self::Symbol(value) => Ok(json!({ "$symbol": value })),
            Self::List(values) => values
                .iter()
                .map(Self::to_json)
                .collect::<AilResult<Vec<_>>>()
                .map(JsonValue::Array),
            Self::Map(values) => {
                let mut document = Map::new();
                for (key, value) in values {
                    document.insert(key.json_name().to_owned(), value.to_json()?);
                }
                Ok(JsonValue::Object(document))
            }
            Self::Ok(value) => Ok(json!({ "ok": value.to_json()? })),
            Self::Err(value) => Ok(json!({ "error": value.to_json()? })),
            _ => Err(Diagnostic::new(
                "RUNTIME_UNSERIALIZABLE_VALUE",
                "runtime value cannot be encoded as JSON",
                json!({ "kind": self.kind() }),
            )),
        }
    }
}

impl From<&Datum> for Value {
    fn from(datum: &Datum) -> Self {
        match &datum.kind {
            DatumKind::Integer(value) => Self::Int(value.clone()),
            DatumKind::Bool(value) => Self::Bool(*value),
            DatumKind::String(value) => Self::String(value.clone()),
            DatumKind::Symbol(value) => Self::Symbol(value.clone()),
            DatumKind::List(values) if values.is_empty() => Self::Nil,
            DatumKind::List(values) => Self::List(values.iter().map(Self::from).collect()),
        }
    }
}

pub fn bigint_json(value: &BigInt) -> JsonValue {
    serde_json::from_str(&value.to_string())
        .unwrap_or_else(|_| JsonValue::String(value.to_string()))
}

pub fn json_to_value(value: &JsonValue) -> AilResult<Value> {
    match value {
        JsonValue::Null => Ok(Value::Nil),
        JsonValue::Bool(value) => Ok(Value::Bool(*value)),
        JsonValue::String(value) => Ok(Value::String(value.clone())),
        JsonValue::Number(value) => BigInt::from_str(&value.to_string())
            .map(Value::Int)
            .map_err(|_| {
                Diagnostic::new(
                    "INPUT_UNSUPPORTED_JSON",
                    "JSON input cannot be converted to a guest value",
                    json!({ "value": value }),
                )
            }),
        JsonValue::Array(values) if values.is_empty() => Ok(Value::Nil),
        JsonValue::Array(values) => values
            .iter()
            .map(json_to_value)
            .collect::<AilResult<Vec<_>>>()
            .map(Value::List),
        JsonValue::Object(values) => {
            let mut mapping = BTreeMap::new();
            for (key, item) in values {
                mapping.insert(MapKey::String(key.clone()), json_to_value(item)?);
            }
            Ok(Value::Map(mapping))
        }
    }
}
