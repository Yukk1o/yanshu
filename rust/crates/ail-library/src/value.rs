#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use num_bigint::BigInt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LibraryKey {
    String(String),
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryValue {
    Nil,
    Bool(bool),
    Int(BigInt),
    String(String),
    Symbol(String),
    List(Vec<Self>),
    Map(BTreeMap<LibraryKey, Self>),
    Ok(Box<Self>),
    Err(Box<Self>),
    Variant {
        type_name: String,
        variant: String,
        fields: Vec<Self>,
    },
}

impl LibraryValue {
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
        }
    }
}
