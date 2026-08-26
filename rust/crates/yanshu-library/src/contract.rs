#![forbid(unsafe_code)]

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::LibraryValue;
use crate::decimal::{format_fuel_work, parse_fuel_work, rescale_fuel_work};
use crate::json::stringify_fuel_work;
use crate::list::{ListOperation, list_fuel_work};
use crate::map::{MapOperation, map_fuel_work};
use crate::math::{checked_clamp_bounds, checked_integer_bits};
use crate::text::{
    checked_case_output_bytes, checked_join_output_bytes, checked_replace_output_bytes,
    checked_split_result, checked_substring_byte_range,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryType {
    Any,
    Nil,
    Bool,
    Int,
    String,
    Symbol,
    List,
    StringList,
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
            Self::StringList => "List<String>",
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
            Self::StringList => match value {
                LibraryValue::Nil => true,
                LibraryValue::List(values) => values
                    .iter()
                    .all(|value| matches!(value, LibraryValue::String(_))),
                _ => false,
            },
            Self::Map => matches!(value, LibraryValue::Map(_)),
            Self::Result => matches!(value, LibraryValue::Ok(_) | LibraryValue::Err(_)),
            Self::Variant => matches!(value, LibraryValue::Variant { .. }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuelModel {
    Fixed(u64),
    TextCharacters {
        base: u64,
        block_size: u64,
    },
    TextReplace {
        base: u64,
        block_size: u64,
    },
    TextCase {
        base: u64,
        block_size: u64,
        uppercase: bool,
    },
    TextSplit {
        base: u64,
        block_size: u64,
    },
    TextJoin {
        base: u64,
        block_size: u64,
    },
    TextSubstring {
        base: u64,
        block_size: u64,
    },
    IntegerLinear {
        base: u64,
        block_size: u64,
    },
    IntegerClamp {
        base: u64,
        block_size: u64,
    },
    IntegerGcd {
        base: u64,
        block_size: u64,
    },
    Utf8Bytes {
        base: u64,
        block_size: u64,
    },
    JsonParse {
        base: u64,
        block_size: u64,
    },
    JsonStringify {
        base: u64,
        block_size: u64,
    },
    DecimalParse {
        base: u64,
        block_size: u64,
    },
    DecimalFormat {
        base: u64,
        block_size: u64,
    },
    DecimalRescale {
        base: u64,
        block_size: u64,
    },
    ListStructural {
        base: u64,
        block_size: u64,
        operation: ListOperation,
    },
    MapStructural {
        base: u64,
        block_size: u64,
        operation: MapOperation,
    },
}

impl FuelModel {
    pub fn cost(self, arguments: &[LibraryValue]) -> YanshuResult<u64> {
        match self {
            Self::Fixed(value) => Ok(value),
            Self::TextCharacters { base, block_size } => {
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
                scaled_cost(base, block_size, characters)
            }
            Self::TextReplace { base, block_size } => {
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
                scaled_cost(base, block_size, work)
            }
            Self::TextCase {
                base,
                block_size,
                uppercase,
            } => {
                let [LibraryValue::String(input)] = arguments else {
                    return Err(invalid_fuel_arguments("text case conversion"));
                };
                let input_characters = u64::try_from(input.chars().count()).unwrap_or(u64::MAX);
                let output_bytes =
                    u64::try_from(checked_case_output_bytes(input, uppercase)?).unwrap_or(u64::MAX);
                scaled_cost(
                    base,
                    block_size,
                    input_characters.saturating_add(output_bytes),
                )
            }
            Self::TextSplit { base, block_size } => {
                let [LibraryValue::String(input), LibraryValue::String(separator)] = arguments
                else {
                    return Err(invalid_fuel_arguments("text split"));
                };
                let metrics = checked_split_result(input, separator)?;
                let work = u64::try_from(input.chars().count())
                    .unwrap_or(u64::MAX)
                    .saturating_add(u64::try_from(separator.chars().count()).unwrap_or(u64::MAX))
                    .saturating_add(u64::try_from(metrics.output_bytes).unwrap_or(u64::MAX))
                    .saturating_add(u64::try_from(metrics.segments).unwrap_or(u64::MAX));
                scaled_cost(base, block_size, work)
            }
            Self::TextJoin { base, block_size } => {
                let [values, LibraryValue::String(separator)] = arguments else {
                    return Err(invalid_fuel_arguments("text join"));
                };
                let values = string_list(values)
                    .ok_or_else(|| invalid_fuel_arguments("text join string list"))?;
                let output_bytes = checked_join_output_bytes(values, separator)?;
                let item_characters = values.iter().fold(0_u64, |total, value| {
                    let LibraryValue::String(value) = value else {
                        return u64::MAX;
                    };
                    total.saturating_add(u64::try_from(value.chars().count()).unwrap_or(u64::MAX))
                });
                let work = item_characters
                    .saturating_add(u64::try_from(separator.chars().count()).unwrap_or(u64::MAX))
                    .saturating_add(u64::try_from(output_bytes).unwrap_or(u64::MAX))
                    .saturating_add(u64::try_from(values.len()).unwrap_or(u64::MAX));
                scaled_cost(base, block_size, work)
            }
            Self::TextSubstring { base, block_size } => {
                let [
                    LibraryValue::String(input),
                    LibraryValue::Int(start),
                    LibraryValue::Int(end),
                ] = arguments
                else {
                    return Err(invalid_fuel_arguments("text substring"));
                };
                let range = checked_substring_byte_range(input, start, end)?;
                let work = u64::try_from(input.chars().count())
                    .unwrap_or(u64::MAX)
                    .saturating_add(
                        u64::try_from(range.end.saturating_sub(range.start)).unwrap_or(u64::MAX),
                    );
                scaled_cost(base, block_size, work)
            }
            Self::IntegerLinear { base, block_size } => {
                let blocks = arguments.iter().try_fold(0_u64, |total, value| {
                    let LibraryValue::Int(value) = value else {
                        return Err(invalid_fuel_arguments("integer linear"));
                    };
                    Ok(total.saturating_add(integer_blocks(value, block_size)?))
                })?;
                Ok(base.saturating_add(blocks))
            }
            Self::IntegerClamp { base, block_size } => {
                let [
                    LibraryValue::Int(value),
                    LibraryValue::Int(minimum),
                    LibraryValue::Int(maximum),
                ] = arguments
                else {
                    return Err(invalid_fuel_arguments("math clamp"));
                };
                checked_clamp_bounds(minimum, maximum)?;
                let blocks = integer_blocks(value, block_size)?
                    .saturating_add(integer_blocks(minimum, block_size)?)
                    .saturating_add(integer_blocks(maximum, block_size)?);
                Ok(base.saturating_add(blocks))
            }
            Self::IntegerGcd { base, block_size } => {
                let [LibraryValue::Int(left), LibraryValue::Int(right)] = arguments else {
                    return Err(invalid_fuel_arguments("math gcd"));
                };
                let left_blocks = integer_blocks(left, block_size)?;
                let right_blocks = integer_blocks(right, block_size)?;
                Ok(base.saturating_add(left_blocks.saturating_mul(right_blocks)))
            }
            Self::Utf8Bytes { base, block_size } => {
                let bytes = arguments.iter().try_fold(0_u64, |total, value| {
                    let LibraryValue::String(value) = value else {
                        return Err(invalid_fuel_arguments("UTF-8 bytes"));
                    };
                    Ok(total.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX)))
                })?;
                scaled_cost(base, block_size, bytes)
            }
            Self::JsonParse { base, block_size } => {
                let [LibraryValue::String(value)] = arguments else {
                    return Err(invalid_fuel_arguments("JSON parse"));
                };
                scaled_cost(
                    base,
                    block_size,
                    u64::try_from(value.len()).unwrap_or(u64::MAX),
                )
            }
            Self::JsonStringify { base, block_size } => {
                let [value] = arguments else {
                    return Err(invalid_fuel_arguments("JSON canonical stringify"));
                };
                scaled_cost(base, block_size, stringify_fuel_work(value))
            }
            Self::DecimalParse { base, block_size } => {
                let [LibraryValue::String(input), LibraryValue::Int(scale)] = arguments else {
                    return Err(invalid_fuel_arguments("decimal scaled parse"));
                };
                scaled_cost(base, block_size, parse_fuel_work(input, scale))
            }
            Self::DecimalFormat { base, block_size } => {
                let [LibraryValue::Int(value), LibraryValue::Int(scale)] = arguments else {
                    return Err(invalid_fuel_arguments("decimal scaled format"));
                };
                scaled_cost(base, block_size, format_fuel_work(value, scale))
            }
            Self::DecimalRescale { base, block_size } => {
                let [
                    LibraryValue::Int(value),
                    LibraryValue::Int(from_scale),
                    LibraryValue::Int(to_scale),
                    LibraryValue::String(rounding),
                ] = arguments
                else {
                    return Err(invalid_fuel_arguments("decimal rescale"));
                };
                scaled_cost(
                    base,
                    block_size,
                    rescale_fuel_work(value, from_scale, to_scale, rounding),
                )
            }
            Self::ListStructural {
                base,
                block_size,
                operation,
            } => scaled_cost(base, block_size, list_fuel_work(operation, arguments)?),
            Self::MapStructural {
                base,
                block_size,
                operation,
            } => scaled_cost(base, block_size, map_fuel_work(operation, arguments)?),
        }
    }
}

fn integer_blocks(value: &num_bigint::BigInt, block_size: u64) -> YanshuResult<u64> {
    if block_size == 0 {
        return Err(Diagnostic::simple(
            "RUNTIME_LIBRARY_CONTRACT_FAILURE",
            "library fuel block size cannot be zero",
        ));
    }
    let bits = checked_integer_bits(value)?;
    Ok(bits.div_ceil(block_size).max(1))
}

fn scaled_cost(base: u64, block_size: u64, work: u64) -> YanshuResult<u64> {
    if block_size == 0 {
        return Err(Diagnostic::simple(
            "RUNTIME_LIBRARY_CONTRACT_FAILURE",
            "library fuel block size cannot be zero",
        ));
    }
    Ok(base.saturating_add(work.saturating_add(block_size - 1) / block_size))
}

fn invalid_fuel_arguments(operation: &str) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_LIBRARY_CONTRACT_FAILURE",
        "library fuel model received invalid arguments",
        json!({ "operation": operation }),
    )
}

fn string_list(value: &LibraryValue) -> Option<&[LibraryValue]> {
    match value {
        LibraryValue::Nil => Some(&[]),
        LibraryValue::List(values) => Some(values),
        _ => None,
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
const ANY: &[LibraryType] = &[LibraryType::Any];
const INT: &[LibraryType] = &[LibraryType::Int];
const INT_INT: &[LibraryType] = &[LibraryType::Int, LibraryType::Int];
const INT_INT_INT: &[LibraryType] = &[LibraryType::Int, LibraryType::Int, LibraryType::Int];
const STRING_INT: &[LibraryType] = &[LibraryType::String, LibraryType::Int];
const STRING_STRING: &[LibraryType] = &[LibraryType::String, LibraryType::String];
const STRING_LIST_STRING: &[LibraryType] = &[LibraryType::StringList, LibraryType::String];
const STRING_INT_INT: &[LibraryType] = &[LibraryType::String, LibraryType::Int, LibraryType::Int];
const THREE_STRINGS: &[LibraryType] = &[
    LibraryType::String,
    LibraryType::String,
    LibraryType::String,
];
const INT_INT_INT_STRING: &[LibraryType] = &[
    LibraryType::Int,
    LibraryType::Int,
    LibraryType::Int,
    LibraryType::String,
];
const LIST: &[LibraryType] = &[LibraryType::List];
const LIST_LIST: &[LibraryType] = &[LibraryType::List, LibraryType::List];
const LIST_ANY: &[LibraryType] = &[LibraryType::List, LibraryType::Any];
const LIST_INT: &[LibraryType] = &[LibraryType::List, LibraryType::Int];
const LIST_INT_INT: &[LibraryType] = &[LibraryType::List, LibraryType::Int, LibraryType::Int];
const MAP: &[LibraryType] = &[LibraryType::Map];
const MAP_ANY: &[LibraryType] = &[LibraryType::Map, LibraryType::Any];
const MAP_MAP: &[LibraryType] = &[LibraryType::Map, LibraryType::Map];

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

const TEXT_V2_OPERATIONS: &[OperationContract] = &[
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
    OperationContract {
        name: "trim",
        parameters: STRING,
        result: LibraryType::String,
        fuel: FuelModel::TextCharacters {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "lowercase",
        parameters: STRING,
        result: LibraryType::String,
        fuel: FuelModel::TextCase {
            base: 1,
            block_size: 64,
            uppercase: false,
        },
    },
    OperationContract {
        name: "uppercase",
        parameters: STRING,
        result: LibraryType::String,
        fuel: FuelModel::TextCase {
            base: 1,
            block_size: 64,
            uppercase: true,
        },
    },
    OperationContract {
        name: "split",
        parameters: STRING_STRING,
        result: LibraryType::StringList,
        fuel: FuelModel::TextSplit {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "join",
        parameters: STRING_LIST_STRING,
        result: LibraryType::String,
        fuel: FuelModel::TextJoin {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "substring",
        parameters: STRING_INT_INT,
        result: LibraryType::String,
        fuel: FuelModel::TextSubstring {
            base: 1,
            block_size: 64,
        },
    },
];

pub const TEXT_V2: LibraryContract = LibraryContract {
    name: "text",
    version: 2,
    operations: TEXT_V2_OPERATIONS,
};

const MATH_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "abs",
        parameters: INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerLinear {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "sign",
        parameters: INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerLinear {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "min",
        parameters: INT_INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerLinear {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "max",
        parameters: INT_INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerLinear {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "clamp",
        parameters: INT_INT_INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerClamp {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "gcd",
        parameters: INT_INT,
        result: LibraryType::Int,
        fuel: FuelModel::IntegerGcd {
            base: 1,
            block_size: 64,
        },
    },
];

pub const MATH_V1: LibraryContract = LibraryContract {
    name: "math",
    version: 1,
    operations: MATH_V1_OPERATIONS,
};

const DIGEST_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "sha256-text",
        parameters: STRING,
        result: LibraryType::String,
        fuel: FuelModel::Utf8Bytes {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "sha512-text",
        parameters: STRING,
        result: LibraryType::String,
        fuel: FuelModel::Utf8Bytes {
            base: 1,
            block_size: 64,
        },
    },
];

pub const DIGEST_V1: LibraryContract = LibraryContract {
    name: "digest",
    version: 1,
    operations: DIGEST_V1_OPERATIONS,
};

const JSON_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "parse",
        parameters: STRING,
        result: LibraryType::Result,
        fuel: FuelModel::JsonParse {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "stringify-canonical",
        parameters: ANY,
        result: LibraryType::Result,
        fuel: FuelModel::JsonStringify {
            base: 1,
            block_size: 64,
        },
    },
];

pub const JSON_V1: LibraryContract = LibraryContract {
    name: "json",
    version: 1,
    operations: JSON_V1_OPERATIONS,
};

const DECIMAL_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "parse-scaled",
        parameters: STRING_INT,
        result: LibraryType::Result,
        fuel: FuelModel::DecimalParse {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "format-scaled",
        parameters: INT_INT,
        result: LibraryType::Result,
        fuel: FuelModel::DecimalFormat {
            base: 1,
            block_size: 64,
        },
    },
    OperationContract {
        name: "rescale",
        parameters: INT_INT_INT_STRING,
        result: LibraryType::Result,
        fuel: FuelModel::DecimalRescale {
            base: 1,
            block_size: 64,
        },
    },
];

pub const DECIMAL_V1: LibraryContract = LibraryContract {
    name: "decimal",
    version: 1,
    operations: DECIMAL_V1_OPERATIONS,
};

const LIST_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "reverse",
        parameters: LIST,
        result: LibraryType::List,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Reverse,
        },
    },
    OperationContract {
        name: "append",
        parameters: LIST_LIST,
        result: LibraryType::List,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Append,
        },
    },
    OperationContract {
        name: "contains?",
        parameters: LIST_ANY,
        result: LibraryType::Bool,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Contains,
        },
    },
    OperationContract {
        name: "get",
        parameters: LIST_INT,
        result: LibraryType::Result,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Get,
        },
    },
    OperationContract {
        name: "take",
        parameters: LIST_INT,
        result: LibraryType::Result,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Take,
        },
    },
    OperationContract {
        name: "drop",
        parameters: LIST_INT,
        result: LibraryType::Result,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Drop,
        },
    },
    OperationContract {
        name: "slice",
        parameters: LIST_INT_INT,
        result: LibraryType::Result,
        fuel: FuelModel::ListStructural {
            base: 1,
            block_size: 64,
            operation: ListOperation::Slice,
        },
    },
];

pub const LIST_V1: LibraryContract = LibraryContract {
    name: "list",
    version: 1,
    operations: LIST_V1_OPERATIONS,
};

const MAP_V1_OPERATIONS: &[OperationContract] = &[
    OperationContract {
        name: "size",
        parameters: MAP,
        result: LibraryType::Int,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::Size,
        },
    },
    OperationContract {
        name: "keys",
        parameters: MAP,
        result: LibraryType::List,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::Keys,
        },
    },
    OperationContract {
        name: "values",
        parameters: MAP,
        result: LibraryType::List,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::Values,
        },
    },
    OperationContract {
        name: "entries",
        parameters: MAP,
        result: LibraryType::List,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::Entries,
        },
    },
    OperationContract {
        name: "contains-value?",
        parameters: MAP_ANY,
        result: LibraryType::Bool,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::ContainsValue,
        },
    },
    OperationContract {
        name: "remove",
        parameters: MAP_ANY,
        result: LibraryType::Result,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::Remove,
        },
    },
    OperationContract {
        name: "merge-disjoint",
        parameters: MAP_MAP,
        result: LibraryType::Result,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::MergeDisjoint,
        },
    },
    OperationContract {
        name: "merge-left",
        parameters: MAP_MAP,
        result: LibraryType::Map,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::MergeLeft,
        },
    },
    OperationContract {
        name: "merge-right",
        parameters: MAP_MAP,
        result: LibraryType::Map,
        fuel: FuelModel::MapStructural {
            base: 1,
            block_size: 64,
            operation: MapOperation::MergeRight,
        },
    },
];

pub const MAP_V1: LibraryContract = LibraryContract {
    name: "map",
    version: 1,
    operations: MAP_V1_OPERATIONS,
};

const TRUSTED_CONTRACTS: &[LibraryContract] = &[
    TEXT_V1, TEXT_V2, MATH_V1, DIGEST_V1, JSON_V1, DECIMAL_V1, LIST_V1, MAP_V1,
];

#[must_use]
pub fn trusted_contract(name: &str, version: u16) -> Option<LibraryContract> {
    TRUSTED_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.name == name && contract.version == version)
}

#[must_use]
pub fn is_trusted_operation_name(public_name: &str) -> bool {
    let Some((library, operation)) = public_name.split_once('/') else {
        return false;
    };
    TRUSTED_CONTRACTS.iter().any(|contract| {
        contract.name == library
            && contract
                .operations
                .iter()
                .any(|candidate| candidate.name == operation)
    })
}
