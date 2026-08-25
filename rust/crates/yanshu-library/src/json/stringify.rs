#![forbid(unsafe_code)]

use crate::{LibraryKey, LibraryValue};

use super::{
    JsonIssue, MAXIMUM_JSON_DEPTH, MAXIMUM_JSON_INTEGER_BITS, MAXIMUM_JSON_NODES,
    MAXIMUM_JSON_OUTPUT_BYTES, MAXIMUM_JSON_STRING_BYTES,
};

pub(super) fn stringify_canonical(value: &LibraryValue) -> Result<String, JsonIssue> {
    let mut measurement = JsonMeasurement::default();
    measurement.measure(value, 1)?;
    let mut output = String::with_capacity(measurement.bytes);
    write_json(value, &mut output)?;
    Ok(output)
}

#[derive(Default)]
struct JsonMeasurement {
    nodes: usize,
    bytes: usize,
}

impl JsonMeasurement {
    fn measure(&mut self, value: &LibraryValue, depth: usize) -> Result<(), JsonIssue> {
        if depth > MAXIMUM_JSON_DEPTH {
            return Err(JsonIssue::limit(
                "JSON_DEPTH_LIMIT",
                None,
                MAXIMUM_JSON_DEPTH,
            ));
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAXIMUM_JSON_NODES {
            return Err(JsonIssue::limit(
                "JSON_NODE_LIMIT",
                None,
                MAXIMUM_JSON_NODES,
            ));
        }
        match value {
            LibraryValue::Nil => self.add_bytes(4),
            LibraryValue::Bool(true) => self.add_bytes(4),
            LibraryValue::Bool(false) => self.add_bytes(5),
            LibraryValue::Int(value) => {
                if value.bits() > MAXIMUM_JSON_INTEGER_BITS {
                    return Err(JsonIssue::limit(
                        "JSON_INTEGER_LIMIT",
                        None,
                        MAXIMUM_JSON_INTEGER_BITS as usize,
                    ));
                }
                self.add_bytes(value.to_string().len())
            }
            LibraryValue::String(value) => self.add_bytes(escaped_string_bytes(value)?),
            LibraryValue::List(values) => {
                self.add_bytes(2)?;
                self.add_bytes(values.len().saturating_sub(1))?;
                for value in values {
                    self.measure(value, depth.saturating_add(1))?;
                }
                Ok(())
            }
            LibraryValue::Map(values) => {
                self.add_bytes(2)?;
                self.add_bytes(values.len().saturating_sub(1))?;
                for (key, value) in values {
                    let LibraryKey::String(key) = key else {
                        return Err(JsonIssue::unsupported("SymbolMapKey"));
                    };
                    self.add_bytes(escaped_string_bytes(key)?)?;
                    self.add_bytes(1)?;
                    self.measure(value, depth.saturating_add(1))?;
                }
                Ok(())
            }
            LibraryValue::Symbol(_) => Err(JsonIssue::unsupported("Symbol")),
            LibraryValue::Ok(_) => Err(JsonIssue::unsupported("Ok")),
            LibraryValue::Err(_) => Err(JsonIssue::unsupported("Err")),
            LibraryValue::Variant { .. } => Err(JsonIssue::unsupported("Variant")),
        }
    }

    fn add_bytes(&mut self, bytes: usize) -> Result<(), JsonIssue> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAXIMUM_JSON_OUTPUT_BYTES {
            Err(JsonIssue::limit(
                "JSON_OUTPUT_LIMIT",
                None,
                MAXIMUM_JSON_OUTPUT_BYTES,
            ))
        } else {
            Ok(())
        }
    }
}

fn escaped_string_bytes(value: &str) -> Result<usize, JsonIssue> {
    if value.len() > MAXIMUM_JSON_STRING_BYTES {
        return Err(JsonIssue::limit(
            "JSON_STRING_LIMIT",
            None,
            MAXIMUM_JSON_STRING_BYTES,
        ));
    }
    let mut bytes = 2_usize;
    for character in value.chars() {
        bytes = bytes.saturating_add(match character {
            '"' | '\\' | '\u{0008}' | '\u{0009}' | '\u{000a}' | '\u{000c}' | '\u{000d}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => character.len_utf8(),
        });
        if bytes > MAXIMUM_JSON_OUTPUT_BYTES {
            return Err(JsonIssue::limit(
                "JSON_OUTPUT_LIMIT",
                None,
                MAXIMUM_JSON_OUTPUT_BYTES,
            ));
        }
    }
    Ok(bytes)
}

fn write_json(value: &LibraryValue, output: &mut String) -> Result<(), JsonIssue> {
    match value {
        LibraryValue::Nil => output.push_str("null"),
        LibraryValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        LibraryValue::Int(value) => output.push_str(&value.to_string()),
        LibraryValue::String(value) => write_json_string(value, output),
        LibraryValue::List(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_json(value, output)?;
            }
            output.push(']');
        }
        LibraryValue::Map(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                let LibraryKey::String(key) = key else {
                    return Err(JsonIssue::unsupported("SymbolMapKey"));
                };
                write_json_string(key, output);
                output.push(':');
                write_json(value, output)?;
            }
            output.push('}');
        }
        LibraryValue::Symbol(_) => return Err(JsonIssue::unsupported("Symbol")),
        LibraryValue::Ok(_) => return Err(JsonIssue::unsupported("Ok")),
        LibraryValue::Err(_) => return Err(JsonIssue::unsupported("Err")),
        LibraryValue::Variant { .. } => return Err(JsonIssue::unsupported("Variant")),
    }
    Ok(())
}

fn write_json_string(value: &str, output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{0008}' => output.push_str("\\b"),
            '\u{0009}' => output.push_str("\\t"),
            '\u{000a}' => output.push_str("\\n"),
            '\u{000c}' => output.push_str("\\f"),
            '\u{000d}' => output.push_str("\\r"),
            '\u{0000}'..='\u{001f}' => {
                let value = character as u8;
                output.push_str("\\u00");
                output.push(char::from(HEX[usize::from(value >> 4)]));
                output.push(char::from(HEX[usize::from(value & 0x0f)]));
            }
            _ => output.push(character),
        }
    }
    output.push('"');
}

pub(crate) fn stringify_fuel_work(value: &LibraryValue) -> u64 {
    const MAXIMUM_FUEL_NODES: usize = MAXIMUM_JSON_NODES + 1;
    let mut work = 0_u64;
    let mut visited = 0_usize;
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > MAXIMUM_FUEL_NODES {
            return u64::MAX;
        }
        work = work.saturating_add(1);
        match value {
            LibraryValue::Nil | LibraryValue::Bool(_) => {}
            LibraryValue::Int(value) => {
                work = work.saturating_add(value.bits().div_ceil(3));
            }
            LibraryValue::String(value) | LibraryValue::Symbol(value) => {
                work = work.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX));
            }
            LibraryValue::List(values) => {
                let remaining = MAXIMUM_FUEL_NODES
                    .saturating_sub(visited)
                    .saturating_sub(stack.len());
                if values.len() > remaining {
                    return u64::MAX;
                }
                stack.extend(values.iter());
            }
            LibraryValue::Map(values) => {
                let remaining = MAXIMUM_FUEL_NODES
                    .saturating_sub(visited)
                    .saturating_sub(stack.len());
                if values.len() > remaining {
                    return u64::MAX;
                }
                for (key, value) in values {
                    let key = match key {
                        LibraryKey::String(key) | LibraryKey::Symbol(key) => key,
                    };
                    work = work.saturating_add(u64::try_from(key.len()).unwrap_or(u64::MAX));
                    stack.push(value);
                }
            }
            LibraryValue::Ok(value) | LibraryValue::Err(value) => stack.push(value),
            LibraryValue::Variant {
                type_name,
                variant,
                fields,
            } => {
                work = work
                    .saturating_add(u64::try_from(type_name.len()).unwrap_or(u64::MAX))
                    .saturating_add(u64::try_from(variant.len()).unwrap_or(u64::MAX));
                let remaining = MAXIMUM_FUEL_NODES
                    .saturating_sub(visited)
                    .saturating_sub(stack.len());
                if fields.len() > remaining {
                    return u64::MAX;
                }
                stack.extend(fields.iter());
            }
        }
    }
    work
}
