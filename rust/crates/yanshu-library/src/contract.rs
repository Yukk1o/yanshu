#![forbid(unsafe_code)]

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::LibraryValue;
use crate::text::checked_replace_output_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryType {
    Any,
    Nil,
    Bool,
    Int,
    String,
    Symbol,
    List,
    Map,
    Result,
    Variant,
}

impl LibraryType {
    #[must_use]
    pub fn display(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Nil => "Nil",
            Self::Bool => "Bool",
            Self::Int => "Int",
            Self::String => "String",
            Self::Symbol => "Symbol",
            Self::List => "List",
            Self::Map => "Map",
            Self::Result => "Result",
            Self::Variant => "Variant",
        }
    }

    #[must_use]
    pub fn accepts(self, value: &LibraryValue) -> bool {
        match self {
            Self::Any => true,
            Self::Nil => matches!(value, LibraryValue::Nil),
            Self::Bool => matches!(value, LibraryValue::Bool(_)),
            Self::Int => matches!(value, LibraryValue::Int(_)),
            Self::String => matches!(value, LibraryValue::String(_)),
            Self::Symbol => matches!(value, LibraryValue::Symbol(_)),
            Self::List => matches!(value, LibraryValue::Nil | LibraryValue::List(_)),
            Self::Map => matches!(value, LibraryValue::Map(_)),
            Self::Result => matches!(value, LibraryValue::Ok(_) | LibraryValue::Err(_)),
            Self::Variant => matches!(value, LibraryValue::Variant { .. }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuelModel {
    Fixed(u64),
    TextCharacters { base: u64, block_size: u64 },
    TextReplace { base: u64, block_size: u64 },
}

impl FuelModel {
    pub fn cost(self, arguments: &[LibraryValue]) -> YanshuResult<u64> {
        match self {
            Self::Fixed(value) => Ok(value),
            Self::TextCharacters { base, block_size } => {
                if block_size == 0 {
                    return Err(Diagnostic::simple(
                        "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                        "library fuel block size cannot be zero",
                    ));
                }
                let characters = arguments.iter().try_fold(0_u64, |total, value| {
                    let LibraryValue::String(text) = value else {
                        return Err(Diagnostic::simple(
                            "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                            "text fuel model received a non-string argument",
                        ));
                    };
                    Ok(total
                        .saturating_add(u64::try_from(text.chars().count()).unwrap_or(u64::MAX)))
                })?;
                Ok(base.saturating_add(characters.saturating_add(block_size - 1) / block_size))
            }
            Self::TextReplace { base, block_size } => {
                if block_size == 0 {
                    return Err(Diagnostic::simple(
                        "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                        "library fuel block size cannot be zero",
                    ));
                }
                let [
                    LibraryValue::String(input),
                    LibraryValue::String(pattern),
                    LibraryValue::String(replacement),
                ] = arguments
                else {
                    return Err(Diagnostic::simple(
                        "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                        "text replacement fuel model received invalid arguments",
                    ));
                };
                let input_characters = u64::try_from(input.chars().count()).unwrap_or(u64::MAX);
                let pattern_characters = u64::try_from(pattern.chars().count()).unwrap_or(u64::MAX);
                let replacement_characters =
                    u64::try_from(replacement.chars().count()).unwrap_or(u64::MAX);
                let output_bytes =
                    u64::try_from(checked_replace_output_bytes(input, pattern, replacement)?)
                        .unwrap_or(u64::MAX);
                let work = input_characters
                    .saturating_add(pattern_characters)
                    .saturating_add(replacement_characters)
                    .saturating_add(output_bytes);
                Ok(base.saturating_add(work.saturating_add(block_size - 1) / block_size))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationContract {
    pub name: &'static str,
    pub parameters: &'static [LibraryType],
    pub result: LibraryType,
    pub fuel: FuelModel,
}

impl OperationContract {
    pub fn validate_arguments(self, arguments: &[LibraryValue]) -> YanshuResult<()> {
        self.validate_arguments_as(self.name, arguments)
    }

    pub fn validate_arguments_as(
        self,
        public_name: &str,
        arguments: &[LibraryValue],
    ) -> YanshuResult<()> {
        if arguments.len() != self.parameters.len() {
            return Err(Diagnostic::new(
                "RUNTIME_ARITY",
                "library function received the wrong number of arguments",
                json!({
                    "name": public_name,
                    "minimum": self.parameters.len(),
                    "maximum": self.parameters.len(),
                    "actual": arguments.len(),
                }),
            ));
        }
        for (index, (expected, actual)) in self.parameters.iter().zip(arguments).enumerate() {
            if !expected.accepts(actual) {
                return Err(Diagnostic::new(
                    "RUNTIME_TYPE",
                    "library function received a value of the wrong type",
                    json!({
                        "operation": public_name,
                        "index": index,
                        "expected": expected.display(),
                        "actual": actual.kind(),
                    }),
                ));
            }
        }
        Ok(())
    }

    pub fn validate_result(self, result: &LibraryValue) -> YanshuResult<()> {
        if self.result.accepts(result) {
            Ok(())
        } else {
            Err(Diagnostic::new(
                "RUNTIME_LIBRARY_INVALID_RESULT",
                "library backend returned a value outside its contract",
                json!({
                    "operation": self.name,
                    "expected": self.result.display(),
                    "actual": result.kind(),
                }),
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryContract {
    pub name: &'static str,
    pub version: u16,
    pub operations: &'static [OperationContract],
}

impl LibraryContract {
    #[must_use]
    pub fn operation(self, name: &str) -> Option<OperationContract> {
        self.operations
            .iter()
            .copied()
            .find(|operation| operation.name == name)
    }
}

const STRING: &[LibraryType] = &[LibraryType::String];
const STRING_STRING: &[LibraryType] = &[LibraryType::String, LibraryType::String];
const THREE_STRINGS: &[LibraryType] = &[
    LibraryType::String,
    LibraryType::String,
    LibraryType::String,
];

const TEXT_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "length",
        parameters: STRING,
        result: LibraryType::Int,
        fuel: FuelModel::TextCharacters {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "starts-with?",
        parameters: STRING_STRING,
        result: LibraryType::Bool,
        fuel: FuelModel::TextCharacters {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "ends-with?",
        parameters: STRING_STRING,
        result: LibraryType::Bool,
        fuel: FuelModel::TextCharacters {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "contains?",
        parameters: STRING_STRING,
        result: LibraryType::Bool,
        fuel: FuelModel::TextCharacters {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "replace",
        parameters: THREE_STRINGS,
        result: LibraryType::String,
        fuel: FuelModel::TextReplace {
            base: 1,
            block_size: 64,
        },
    },
];

pub const TEXT_V1: LibraryContract = LibraryContract {
    name: "text",
    version: 1,
    operations: TEXT_OPERATIONS,
};

#[must_use]
pub fn trusted_contract(name: &str, version: u16) -> Option<LibraryContract> {
    match (name, version) {
        ("text", 1) => Some(TEXT_V1),
        _ => None,
    }
}
