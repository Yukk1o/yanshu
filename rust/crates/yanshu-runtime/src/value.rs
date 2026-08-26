#![forbid(unsafe_code)]

use std::{collections::BTreeMap, str::FromStr};

use num_bigint::BigInt;
use serde_json::{Map, Value as JsonValue, json};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_library::{
    MAXIMUM_LIBRARY_INTEGER_BITS, MAXIMUM_LIBRARY_VALUE_BYTES, MAXIMUM_LIBRARY_VALUE_DEPTH,
    MAXIMUM_LIBRARY_VALUE_NODES,
};
use yanshu_syntax::{Datum, DatumKind, SchemaKind};

pub const MAXIMUM_VALUE_NODES: usize = MAXIMUM_LIBRARY_VALUE_NODES;
pub const MAXIMUM_VALUE_DEPTH: usize = MAXIMUM_LIBRARY_VALUE_DEPTH;
pub const MAXIMUM_VALUE_BYTES: usize = MAXIMUM_LIBRARY_VALUE_BYTES;
pub const MAXIMUM_INTEGER_BITS: u64 = MAXIMUM_LIBRARY_INTEGER_BITS;
pub const MAXIMUM_INTEGER_DECIMAL_DIGITS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValueMetrics {
    pub(crate) nodes: u64,
    pub(crate) scalar_bytes: u64,
    pub(crate) integer_bits: u64,
}

impl ValueMetrics {
    #[must_use]
    pub(crate) fn fuel_cost(self) -> u64 {
        self.nodes
            .saturating_add(self.scalar_bytes.div_ceil(64))
            .saturating_add(self.integer_bits.div_ceil(64))
    }
}

#[derive(Default)]
struct ValueMeasure {
    nodes: usize,
    scalar_bytes: usize,
    integer_bits: u64,
}

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
    Variant {
        type_name: String,
        variant: String,
        fields: Vec<Self>,
    },
    Constructor {
        type_name: String,
        variant: String,
        arity: usize,
    },
    Schema {
        name: String,
        specification: SchemaKind,
    },
    Closure(usize),
    Primitive(Primitive),
    LibraryFunction {
        name: String,
        library: String,
        version: u16,
        operation: String,
        arity: usize,
    },
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
    CheckedQuotient,
    CheckedRemainder,
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
    NumberToString,
    List,
    ListMap,
    ListFilter,
    ListFold,
    Sum,
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
    ValidateReport,
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
            Self::Variant { .. } => "Variant",
            Self::Constructor { .. } => "Constructor",
            Self::Schema { .. } => "Schema",
            Self::Closure(_) => "Function",
            Self::Primitive(_) => "Primitive",
            Self::LibraryFunction { .. } => "LibraryFunction",
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
            Self::Variant {
                type_name,
                variant,
                fields,
            } => format!(
                "({}/{variant}{})",
                type_name,
                fields
                    .iter()
                    .map(|field| format!(" {}", field.display()))
                    .collect::<String>()
            ),
            Self::Constructor { variant, .. } => format!("#<constructor:{variant}>"),
            Self::Schema { name, .. } => format!("#<schema:{name}>"),
            Self::Closure(_) => "#<function>".to_owned(),
            Self::Primitive(primitive) => format!("#<primitive:{}>", primitive.name),
            Self::LibraryFunction { name, .. } => format!("#<library-function:{name}>"),
        }
    }

    pub fn to_json(&self) -> YanshuResult<JsonValue> {
        match self {
            Self::Nil => Ok(JsonValue::Array(Vec::new())),
            Self::Bool(value) => Ok(JsonValue::Bool(*value)),
            Self::Int(value) => Ok(bigint_json(value)),
            Self::String(value) => Ok(JsonValue::String(value.clone())),
            Self::Symbol(value) => Ok(json!({ "$symbol": value })),
            Self::List(values) => values
                .iter()
                .map(Self::to_json)
                .collect::<YanshuResult<Vec<_>>>()
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
            Self::Variant {
                type_name,
                variant,
                fields,
            } => Ok(json!({
                "$type": type_name,
                "$variant": variant,
                "fields": fields
                    .iter()
                    .map(Self::to_json)
                    .collect::<YanshuResult<Vec<_>>>()?,
            })),
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

pub fn json_to_value(value: &JsonValue) -> YanshuResult<Value> {
    let mut measure = ValueMeasure::default();
    json_to_value_at(value, 0, &mut measure)
}

fn json_to_value_at(
    value: &JsonValue,
    depth: usize,
    measure: &mut ValueMeasure,
) -> YanshuResult<Value> {
    measure.bump_node(depth)?;
    match value {
        JsonValue::Null => Ok(Value::Nil),
        JsonValue::Bool(value) => Ok(Value::Bool(*value)),
        JsonValue::String(value) => {
            measure.add_bytes(value.len())?;
            Ok(Value::String(value.clone()))
        }
        JsonValue::Number(value) => {
            let encoded = value.to_string();
            if encoded.len() > MAXIMUM_INTEGER_DECIMAL_DIGITS {
                return Err(value_limit(
                    "integerDigits",
                    MAXIMUM_INTEGER_DECIMAL_DIGITS,
                    encoded.len(),
                ));
            }
            let integer = BigInt::from_str(&encoded).map_err(|_| {
                Diagnostic::new(
                    "INPUT_UNSUPPORTED_JSON",
                    "JSON input cannot be converted to a guest value",
                    json!({ "value": value }),
                )
            })?;
            measure.add_integer(&integer)?;
            Ok(Value::Int(integer))
        }
        JsonValue::Array(values) if values.is_empty() => Ok(Value::Nil),
        JsonValue::Array(values) => {
            measure.check_collection(values.len())?;
            values
                .iter()
                .map(|item| json_to_value_at(item, depth + 1, measure))
                .collect::<YanshuResult<Vec<_>>>()
                .map(Value::List)
        }
        JsonValue::Object(values) => {
            measure.check_collection(values.len())?;
            let mut mapping = BTreeMap::new();
            for (key, item) in values {
                measure.add_bytes(key.len())?;
                mapping.insert(
                    MapKey::String(key.clone()),
                    json_to_value_at(item, depth + 1, measure)?,
                );
            }
            Ok(Value::Map(mapping))
        }
    }
}

pub(crate) fn measure_runtime_value(value: &Value) -> YanshuResult<ValueMetrics> {
    let mut measure = ValueMeasure::default();
    measure.visit_value(value, 0, false)?;
    Ok(measure.metrics())
}

pub(crate) fn measure_portable_value(value: &Value) -> YanshuResult<ValueMetrics> {
    let mut measure = ValueMeasure::default();
    measure.visit_value(value, 0, true)?;
    Ok(measure.metrics())
}

pub(crate) fn measure_datum(datum: &Datum) -> YanshuResult<ValueMetrics> {
    let mut measure = ValueMeasure::default();
    measure.visit_datum(datum, 0)?;
    Ok(measure.metrics())
}

impl ValueMeasure {
    fn metrics(&self) -> ValueMetrics {
        ValueMetrics {
            nodes: u64::try_from(self.nodes).unwrap_or(u64::MAX),
            scalar_bytes: u64::try_from(self.scalar_bytes).unwrap_or(u64::MAX),
            integer_bits: self.integer_bits,
        }
    }

    fn bump_node(&mut self, depth: usize) -> YanshuResult<()> {
        if depth > MAXIMUM_VALUE_DEPTH {
            return Err(value_limit("depth", MAXIMUM_VALUE_DEPTH, depth));
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAXIMUM_VALUE_NODES {
            return Err(value_limit("nodes", MAXIMUM_VALUE_NODES, self.nodes));
        }
        Ok(())
    }

    fn check_collection(&self, length: usize) -> YanshuResult<()> {
        if length > MAXIMUM_VALUE_NODES {
            return Err(value_limit("collectionItems", MAXIMUM_VALUE_NODES, length));
        }
        Ok(())
    }

    fn add_bytes(&mut self, bytes: usize) -> YanshuResult<()> {
        self.scalar_bytes = self.scalar_bytes.saturating_add(bytes);
        if self.scalar_bytes > MAXIMUM_VALUE_BYTES {
            return Err(value_limit(
                "scalarBytes",
                MAXIMUM_VALUE_BYTES,
                self.scalar_bytes,
            ));
        }
        Ok(())
    }

    fn add_integer(&mut self, value: &BigInt) -> YanshuResult<()> {
        let bits = value.bits();
        if bits > MAXIMUM_INTEGER_BITS {
            return Err(Diagnostic::new(
                "RUNTIME_VALUE_LIMIT",
                "guest value exceeds a structural resource limit",
                json!({
                    "kind": "integerBits",
                    "maximum": MAXIMUM_INTEGER_BITS,
                    "actual": bits,
                }),
            ));
        }
        self.integer_bits = self.integer_bits.saturating_add(bits);
        self.add_bytes(usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX))
    }

    fn visit_value(&mut self, value: &Value, depth: usize, portable: bool) -> YanshuResult<()> {
        self.bump_node(depth)?;
        match value {
            Value::Nil | Value::Bool(_) => Ok(()),
            Value::Int(value) => self.add_integer(value),
            Value::String(value) | Value::Symbol(value) => self.add_bytes(value.len()),
            Value::List(values) => {
                self.check_collection(values.len())?;
                for value in values {
                    self.visit_value(value, depth + 1, portable)?;
                }
                Ok(())
            }
            Value::Map(values) => {
                self.check_collection(values.len())?;
                for (key, value) in values {
                    self.add_bytes(key.json_name().len())?;
                    self.visit_value(value, depth + 1, portable)?;
                }
                Ok(())
            }
            Value::Ok(value) | Value::Err(value) => self.visit_value(value, depth + 1, portable),
            Value::Variant {
                type_name,
                variant,
                fields,
            } => {
                self.add_bytes(type_name.len().saturating_add(variant.len()))?;
                self.check_collection(fields.len())?;
                for field in fields {
                    self.visit_value(field, depth + 1, portable)?;
                }
                Ok(())
            }
            Value::Constructor {
                type_name, variant, ..
            } => {
                if portable {
                    return Err(non_portable_value(value));
                }
                self.add_bytes(type_name.len().saturating_add(variant.len()))
            }
            Value::Closure(_) | Value::Primitive(_) => {
                if portable {
                    Err(non_portable_value(value))
                } else {
                    Ok(())
                }
            }
            Value::Schema {
                name,
                specification,
            } => {
                if portable {
                    return Err(non_portable_value(value));
                }
                self.add_bytes(name.len())?;
                self.visit_schema(specification, depth + 1)
            }
            Value::LibraryFunction {
                name,
                library,
                operation,
                ..
            } => {
                if portable {
                    return Err(non_portable_value(value));
                }
                self.add_bytes(
                    name.len()
                        .saturating_add(library.len())
                        .saturating_add(operation.len()),
                )
            }
        }
    }

    fn visit_schema(&mut self, schema: &SchemaKind, depth: usize) -> YanshuResult<()> {
        self.bump_node(depth)?;
        match schema {
            SchemaKind::Any | SchemaKind::Boolean => Ok(()),
            SchemaKind::Enum { values } => {
                self.check_collection(values.len())?;
                for value in values {
                    self.visit_datum(value, depth + 1)?;
                }
                Ok(())
            }
            SchemaKind::Union { variants } => {
                self.check_collection(variants.len())?;
                for variant in variants {
                    self.visit_schema(variant, depth + 1)?;
                }
                Ok(())
            }
            SchemaKind::String {
                minimum_length,
                maximum_length,
            } => {
                self.add_integer(minimum_length)?;
                if let Some(maximum) = maximum_length {
                    self.add_integer(maximum)?;
                }
                Ok(())
            }
            SchemaKind::Integer { minimum, maximum } => {
                if let Some(minimum) = minimum {
                    self.add_integer(minimum)?;
                }
                if let Some(maximum) = maximum {
                    self.add_integer(maximum)?;
                }
                Ok(())
            }
            SchemaKind::List { item, .. } => self.visit_schema(item, depth + 1),
            SchemaKind::Object { fields } => {
                self.check_collection(fields.len())?;
                for field in fields {
                    self.bump_node(depth + 1)?;
                    self.add_bytes(field.name.len())?;
                    self.visit_schema(&field.specification, depth + 2)?;
                    if let Some(default) = &field.default {
                        self.visit_datum(default, depth + 2)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn visit_datum(&mut self, datum: &Datum, depth: usize) -> YanshuResult<()> {
        self.bump_node(depth)?;
        match &datum.kind {
            DatumKind::Integer(value) => self.add_integer(value),
            DatumKind::Bool(_) => Ok(()),
            DatumKind::String(value) | DatumKind::Symbol(value) => self.add_bytes(value.len()),
            DatumKind::List(values) => {
                self.check_collection(values.len())?;
                for value in values {
                    self.visit_datum(value, depth + 1)?;
                }
                Ok(())
            }
        }
    }
}

fn value_limit(kind: &'static str, maximum: usize, actual: usize) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_VALUE_LIMIT",
        "guest value exceeds a structural resource limit",
        json!({ "kind": kind, "maximum": maximum, "actual": actual }),
    )
}

fn non_portable_value(value: &Value) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_NON_PORTABLE_VALUE",
        "host boundary returned a non-portable runtime value",
        json!({ "kind": value.kind() }),
    )
}
