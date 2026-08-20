#![forbid(unsafe_code)]

use serde_json::{Value, json};
use yanshu_syntax::TypeExpression;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Any,
    Integer,
    Boolean,
    String,
    Symbol,
    Nil,
    List(Box<Self>),
    Map,
    Result {
        success: Box<Self>,
        error: Box<Self>,
    },
    User(String),
    Schema(Box<Self>),
    Function {
        parameters: Vec<Self>,
        result: Box<Self>,
    },
    Variable(u32),
}

impl Type {
    #[must_use]
    pub fn from_expression(expression: &TypeExpression) -> Self {
        match expression {
            TypeExpression::Named(name) => match name.as_str() {
                "any" => Self::Any,
                "integer" => Self::Integer,
                "boolean" => Self::Boolean,
                "string" => Self::String,
                "symbol" => Self::Symbol,
                "nil" => Self::Nil,
                "map" => Self::Map,
                _ => Self::User(name.clone()),
            },
            TypeExpression::List(item) => Self::List(Box::new(Self::from_expression(item))),
            TypeExpression::Result { success, error } => Self::Result {
                success: Box::new(Self::from_expression(success)),
                error: Box::new(Self::from_expression(error)),
            },
            TypeExpression::Function { parameters, result } => Self::Function {
                parameters: parameters.iter().map(Self::from_expression).collect(),
                result: Box::new(Self::from_expression(result)),
            },
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Any => "Any".to_owned(),
            Self::Integer => "Int".to_owned(),
            Self::Boolean => "Bool".to_owned(),
            Self::String => "String".to_owned(),
            Self::Symbol => "Symbol".to_owned(),
            Self::Nil => "Nil".to_owned(),
            Self::List(item) => format!("List<{}>", item.display()),
            Self::Map => "Map".to_owned(),
            Self::Result { success, error } => {
                format!("Result<{}, {}>", success.display(), error.display())
            }
            Self::User(name) => name.clone(),
            Self::Schema(item) => format!("Schema<{}>", item.display()),
            Self::Function { parameters, result } => format!(
                "fn({}) -> {}",
                parameters
                    .iter()
                    .map(Self::display)
                    .collect::<Vec<_>>()
                    .join(", "),
                result.display()
            ),
            Self::Variable(identifier) => format!("T{identifier}"),
        }
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Any => json!({ "type": "any" }),
            Self::Integer => json!({ "type": "integer" }),
            Self::Boolean => json!({ "type": "boolean" }),
            Self::String => json!({ "type": "string" }),
            Self::Symbol => json!({ "type": "symbol" }),
            Self::Nil => json!({ "type": "nil" }),
            Self::List(item) => json!({ "type": "list", "item": item.to_json() }),
            Self::Map => json!({ "type": "map" }),
            Self::Result { success, error } => json!({
                "type": "result",
                "success": success.to_json(),
                "error": error.to_json(),
            }),
            Self::User(name) => json!({ "type": "user", "name": name }),
            Self::Schema(item) => json!({ "type": "schema", "item": item.to_json() }),
            Self::Function { parameters, result } => json!({
                "type": "function",
                "parameters": parameters.iter().map(Self::to_json).collect::<Vec<_>>(),
                "result": result.to_json(),
            }),
            Self::Variable(identifier) => json!({ "type": "inferred", "id": identifier }),
        }
    }
}
