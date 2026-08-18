#![forbid(unsafe_code)]

use ail_diagnostic::Span;
use num_bigint::BigInt;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datum {
    pub kind: DatumKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatumKind {
    Integer(BigInt),
    Bool(bool),
    String(String),
    Symbol(String),
    List(Vec<Datum>),
}

impl Datum {
    #[must_use]
    pub fn symbol(&self) -> Option<&str> {
        match &self.kind {
            DatumKind::Symbol(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn list(&self) -> Option<&[Self]> {
        match &self.kind {
            DatumKind::List(values) => Some(values),
            _ => None,
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match &self.kind {
            DatumKind::Integer(value) => value.to_string(),
            DatumKind::Bool(true) => "#t".to_owned(),
            DatumKind::Bool(false) => "#f".to_owned(),
            DatumKind::String(value) => format!("{value:?}"),
            DatumKind::Symbol(value) => value.clone(),
            DatumKind::List(values) => {
                let body = values
                    .iter()
                    .map(Self::display)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("({body})")
            }
        }
    }

    #[must_use]
    pub fn portable_json(&self) -> Value {
        match &self.kind {
            DatumKind::Integer(value) => bigint_json(value),
            DatumKind::Bool(value) => Value::Bool(*value),
            DatumKind::String(value) => Value::String(value.clone()),
            DatumKind::Symbol(value) => json!({ "$symbol": value }),
            DatumKind::List(values) => {
                Value::Array(values.iter().map(Self::portable_json).collect())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub name: String,
    pub version: BigInt,
    pub imports: Vec<String>,
    pub capabilities: Vec<String>,
    pub libraries: Vec<LibraryRequirement>,
    pub data_types: Vec<DataTypeDefinition>,
    pub signatures: Vec<FunctionSignature>,
    pub type_exports: Vec<String>,
    pub schemas: Vec<Schema>,
    pub routes: Vec<Route>,
    pub definitions: Vec<Definition>,
    pub exports: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTypeDefinition {
    pub name: String,
    pub variants: Vec<VariantDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDefinition {
    pub name: String,
    pub fields: Vec<DataField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataField {
    pub name: String,
    pub type_expression: Option<TypeExpression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    pub name: String,
    pub parameters: Vec<TypeExpression>,
    pub result: TypeExpression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpression {
    Named(String),
    List(Box<Self>),
    Result {
        success: Box<Self>,
        error: Box<Self>,
    },
    Function {
        parameters: Vec<Self>,
        result: Box<Self>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRequirement {
    pub name: String,
    pub version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub kind: SchemaKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaKind {
    Any,
    Enum {
        values: Vec<Datum>,
    },
    Union {
        variants: Vec<Self>,
    },
    String {
        minimum_length: BigInt,
        maximum_length: Option<BigInt>,
    },
    Integer {
        minimum: Option<BigInt>,
        maximum: Option<BigInt>,
    },
    Boolean,
    List {
        item: Box<Self>,
        minimum_length: u64,
        maximum_length: u64,
    },
    Object {
        fields: Vec<SchemaField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    pub name: String,
    pub specification: SchemaKind,
    pub required: bool,
    pub default: Option<Datum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionKind {
    Literal(Datum),
    Variable(String),
    Quote(Datum),
    If {
        condition: Box<Expression>,
        consequent: Box<Expression>,
        alternative: Box<Expression>,
    },
    And(Vec<Expression>),
    Or(Vec<Expression>),
    Cond {
        clauses: Vec<CondClause>,
        alternative: Box<Expression>,
    },
    Match {
        value: Box<Expression>,
        arms: Vec<MatchArm>,
    },
    Let {
        bindings: Vec<Binding>,
        body: Box<Expression>,
    },
    Function {
        parameters: Vec<String>,
        body: Box<Expression>,
    },
    Do(Vec<Expression>),
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondClause {
    pub condition: Expression,
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternKind {
    Wildcard,
    Binding(String),
    Literal(Datum),
    Variant { name: String, fields: Vec<Pattern> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub expression: Expression,
}

impl Program {
    #[must_use]
    pub fn summary_json(&self) -> Value {
        let mut document = json!({
            "name": self.name,
            "version": bigint_json(&self.version),
            "capabilities": self.capabilities,
            "libraries": self.libraries.iter().map(LibraryRequirement::to_json).collect::<Vec<_>>(),
            "schemas": self.schemas.iter().map(|schema| schema.name.clone()).collect::<Vec<_>>(),
            "routes": self.routes.iter().map(Route::to_json).collect::<Vec<_>>(),
            "definitions": self.definitions.iter().map(|definition| definition.name.clone()).collect::<Vec<_>>(),
            "exports": self.exports,
        });
        if let Value::Object(fields) = &mut document {
            if !self.imports.is_empty() {
                fields.insert("imports".to_owned(), json!(self.imports));
            }
            if !self.data_types.is_empty() {
                fields.insert(
                    "dataTypes".to_owned(),
                    Value::Array(
                        self.data_types
                            .iter()
                            .map(DataTypeDefinition::to_json)
                            .collect(),
                    ),
                );
            }
            if !self.signatures.is_empty() {
                fields.insert(
                    "signatures".to_owned(),
                    Value::Array(
                        self.signatures
                            .iter()
                            .map(FunctionSignature::to_json)
                            .collect(),
                    ),
                );
            }
            if !self.type_exports.is_empty() {
                fields.insert("typeExports".to_owned(), json!(self.type_exports));
            }
        }
        document
    }

    #[must_use]
    pub fn inspect_json(&self) -> Value {
        let mut document = json!({
            "type": "program",
            "name": self.name,
            "version": bigint_json(&self.version),
            "capabilities": self.capabilities,
            "libraries": self.libraries.iter().map(LibraryRequirement::to_json).collect::<Vec<_>>(),
            "schemas": self.schemas.iter().map(Schema::to_json).collect::<Vec<_>>(),
            "routes": self.routes.iter().map(Route::to_json).collect::<Vec<_>>(),
            "definitions": self.definitions.iter().map(Definition::to_json).collect::<Vec<_>>(),
            "exports": self.exports,
        });
        if let Value::Object(fields) = &mut document {
            if !self.imports.is_empty() {
                fields.insert("imports".to_owned(), json!(self.imports));
            }
            if !self.data_types.is_empty() {
                fields.insert(
                    "dataTypes".to_owned(),
                    Value::Array(
                        self.data_types
                            .iter()
                            .map(DataTypeDefinition::to_json)
                            .collect(),
                    ),
                );
            }
            if !self.signatures.is_empty() {
                fields.insert(
                    "signatures".to_owned(),
                    Value::Array(
                        self.signatures
                            .iter()
                            .map(FunctionSignature::to_json)
                            .collect(),
                    ),
                );
            }
            if !self.type_exports.is_empty() {
                fields.insert("typeExports".to_owned(), json!(self.type_exports));
            }
        }
        document
    }
}

impl DataTypeDefinition {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "variants": self.variants.iter().map(VariantDefinition::to_json).collect::<Vec<_>>(),
        })
    }
}

impl VariantDefinition {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "fields": self.fields.iter().map(DataField::to_json).collect::<Vec<_>>(),
        })
    }
}

impl DataField {
    fn to_json(&self) -> Value {
        let mut document = json!({ "name": self.name });
        if let Some(type_expression) = &self.type_expression {
            document["type"] = type_expression.to_json();
        }
        document
    }
}

impl FunctionSignature {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "parameters": self.parameters.iter().map(TypeExpression::to_json).collect::<Vec<_>>(),
            "result": self.result.to_json(),
        })
    }
}

impl TypeExpression {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Named(name) => json!({ "type": "named", "name": name }),
            Self::List(item) => json!({ "type": "list", "item": item.to_json() }),
            Self::Result { success, error } => json!({
                "type": "result",
                "success": success.to_json(),
                "error": error.to_json(),
            }),
            Self::Function { parameters, result } => json!({
                "type": "function",
                "parameters": parameters.iter().map(Self::to_json).collect::<Vec<_>>(),
                "result": result.to_json(),
            }),
        }
    }
}

impl LibraryRequirement {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "version": self.version })
    }
}

impl Route {
    fn to_json(&self) -> Value {
        json!({ "method": self.method, "path": self.path, "handler": self.handler })
    }
}

impl Schema {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "schema": self.kind.to_json() })
    }
}

impl SchemaKind {
    fn to_json(&self) -> Value {
        match self {
            Self::Any => json!({ "type": "any" }),
            Self::Enum { values } => json!({
                "type": "enum",
                "values": values.iter().map(Datum::portable_json).collect::<Vec<_>>(),
            }),
            Self::Union { variants } => json!({
                "type": "union",
                "variants": variants.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            Self::String {
                minimum_length,
                maximum_length,
            } => json!({
                "type": "string",
                "minimumLength": bigint_json(minimum_length),
                "maximumLength": maximum_length.as_ref().map_or(Value::Bool(false), bigint_json),
            }),
            Self::Integer { minimum, maximum } => json!({
                "type": "integer",
                "minimum": minimum.as_ref().map_or(Value::Bool(false), bigint_json),
                "maximum": maximum.as_ref().map_or(Value::Bool(false), bigint_json),
            }),
            Self::Boolean => json!({ "type": "boolean" }),
            Self::List {
                item,
                minimum_length,
                maximum_length,
            } => json!({
                "type": "list",
                "item": item.to_json(),
                "minimumLength": minimum_length,
                "maximumLength": maximum_length,
            }),
            Self::Object { fields } => json!({
                "type": "object",
                "additionalProperties": false,
                "fields": fields.iter().map(SchemaField::to_json).collect::<Vec<_>>(),
            }),
        }
    }
}

impl SchemaField {
    fn to_json(&self) -> Value {
        let mut document = Map::new();
        document.insert("name".to_owned(), Value::String(self.name.clone()));
        document.insert("required".to_owned(), Value::Bool(self.required));
        document.insert("schema".to_owned(), self.specification.to_json());
        if let Some(default) = &self.default {
            document.insert("default".to_owned(), default.portable_json());
        }
        Value::Object(document)
    }
}

impl Definition {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "expression": self.expression.to_json() })
    }
}

impl Expression {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match &self.kind {
            ExpressionKind::Literal(value) => {
                json!({ "type": "literal", "value": value.portable_json() })
            }
            ExpressionKind::Variable(name) => json!({ "type": "variable", "name": name }),
            ExpressionKind::Quote(datum) => {
                json!({ "type": "quote", "datum": datum.portable_json() })
            }
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => json!({
                "type": "if",
                "condition": condition.to_json(),
                "consequent": consequent.to_json(),
                "alternative": alternative.to_json(),
            }),
            ExpressionKind::And(expressions) => json!({
                "type": "and",
                "expressions": expressions.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            ExpressionKind::Or(expressions) => json!({
                "type": "or",
                "expressions": expressions.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => json!({
                "type": "cond",
                "clauses": clauses.iter().map(CondClause::to_json).collect::<Vec<_>>(),
                "alternative": alternative.to_json(),
            }),
            ExpressionKind::Match { value, arms } => json!({
                "type": "match",
                "value": value.to_json(),
                "arms": arms.iter().map(MatchArm::to_json).collect::<Vec<_>>(),
            }),
            ExpressionKind::Let { bindings, body } => json!({
                "type": "let",
                "bindings": bindings.iter().map(Binding::to_json).collect::<Vec<_>>(),
                "body": body.to_json(),
            }),
            ExpressionKind::Function { parameters, body } => json!({
                "type": "function",
                "parameters": parameters,
                "body": body.to_json(),
            }),
            ExpressionKind::Do(expressions) => json!({
                "type": "do",
                "expressions": expressions.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            ExpressionKind::Call { callee, arguments } => json!({
                "type": "call",
                "callee": callee.to_json(),
                "arguments": arguments.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
        }
    }
}

impl Binding {
    fn to_json(&self) -> Value {
        json!({ "name": self.name, "expression": self.expression.to_json() })
    }
}

impl CondClause {
    fn to_json(&self) -> Value {
        json!({
            "condition": self.condition.to_json(),
            "expression": self.expression.to_json(),
        })
    }
}

impl MatchArm {
    fn to_json(&self) -> Value {
        json!({
            "pattern": self.pattern.to_json(),
            "expression": self.expression.to_json(),
        })
    }
}

impl Pattern {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match &self.kind {
            PatternKind::Wildcard => json!({ "type": "wildcard" }),
            PatternKind::Binding(name) => json!({ "type": "binding", "name": name }),
            PatternKind::Literal(value) => {
                json!({ "type": "literal", "value": value.portable_json() })
            }
            PatternKind::Variant { name, fields } => json!({
                "type": "variant",
                "name": name,
                "fields": fields.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
        }
    }
}

fn bigint_json(value: &BigInt) -> Value {
    serde_json::from_str(&value.to_string()).unwrap_or_else(|_| Value::String(value.to_string()))
}
