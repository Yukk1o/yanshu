#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use num_bigint::BigInt;

use crate::{LibraryKey, LibraryValue};

use super::{
    JsonIssue, MAXIMUM_JSON_DEPTH, MAXIMUM_JSON_INPUT_BYTES, MAXIMUM_JSON_INTEGER_BITS,
    MAXIMUM_JSON_INTEGER_DIGITS, MAXIMUM_JSON_NODES, MAXIMUM_JSON_STRING_BYTES,
};

pub(super) fn parse_json(input: &str) -> Result<LibraryValue, JsonIssue> {
    if input.len() > MAXIMUM_JSON_INPUT_BYTES {
        return Err(JsonIssue::limit(
            "JSON_INPUT_LIMIT",
            None,
            MAXIMUM_JSON_INPUT_BYTES,
        ));
    }
    Parser::new(input).parse_document()
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            offset: 0,
            nodes: 0,
        }
    }

    fn parse_document(mut self) -> Result<LibraryValue, JsonIssue> {
        self.skip_whitespace();
        let value = self.parse_value(1)?;
        self.skip_whitespace();
        if self.offset != self.bytes.len() {
            return Err(self.issue("JSON_SYNTAX"));
        }
        Ok(value)
    }

    fn parse_value(&mut self, depth: usize) -> Result<LibraryValue, JsonIssue> {
        if depth > MAXIMUM_JSON_DEPTH {
            return Err(JsonIssue::limit(
                "JSON_DEPTH_LIMIT",
                Some(self.offset),
                MAXIMUM_JSON_DEPTH,
            ));
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAXIMUM_JSON_NODES {
            return Err(JsonIssue::limit(
                "JSON_NODE_LIMIT",
                Some(self.offset),
                MAXIMUM_JSON_NODES,
            ));
        }
        match self.peek() {
            Some(b'n') => {
                self.consume_literal(b"null")?;
                Ok(LibraryValue::Nil)
            }
            Some(b't') => {
                self.consume_literal(b"true")?;
                Ok(LibraryValue::Bool(true))
            }
            Some(b'f') => {
                self.consume_literal(b"false")?;
                Ok(LibraryValue::Bool(false))
            }
            Some(b'"') => self.parse_string().map(LibraryValue::String),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(b'-' | b'0'..=b'9') => self.parse_integer(),
            _ => Err(self.issue("JSON_SYNTAX")),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<LibraryValue, JsonIssue> {
        self.offset += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.consume_if(b']') {
            return Ok(LibraryValue::List(values));
        }
        loop {
            values.push(self.parse_value(depth.saturating_add(1))?);
            self.skip_whitespace();
            if self.consume_if(b']') {
                return Ok(LibraryValue::List(values));
            }
            if !self.consume_if(b',') {
                return Err(self.issue("JSON_SYNTAX"));
            }
            self.skip_whitespace();
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<LibraryValue, JsonIssue> {
        self.offset += 1;
        self.skip_whitespace();
        let mut values = BTreeMap::new();
        if self.consume_if(b'}') {
            return Ok(LibraryValue::Map(values));
        }
        loop {
            let key_offset = self.offset;
            if self.peek() != Some(b'"') {
                return Err(self.issue("JSON_SYNTAX"));
            }
            let key = self.parse_string()?;
            let map_key = LibraryKey::String(key);
            if values.contains_key(&map_key) {
                return Err(JsonIssue::at("JSON_DUPLICATE_KEY", key_offset));
            }
            self.skip_whitespace();
            if !self.consume_if(b':') {
                return Err(self.issue("JSON_SYNTAX"));
            }
            self.skip_whitespace();
            let value = self.parse_value(depth.saturating_add(1))?;
            values.insert(map_key, value);
            self.skip_whitespace();
            if self.consume_if(b'}') {
                return Ok(LibraryValue::Map(values));
            }
            if !self.consume_if(b',') {
                return Err(self.issue("JSON_SYNTAX"));
            }
            self.skip_whitespace();
        }
    }

    fn parse_integer(&mut self) -> Result<LibraryValue, JsonIssue> {
        let start = self.offset;
        self.consume_if(b'-');
        match self.peek() {
            Some(b'0') => {
                self.offset += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.issue("JSON_SYNTAX"));
                }
            }
            Some(b'1'..=b'9') => {
                self.offset += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.offset += 1;
                }
            }
            _ => return Err(self.issue("JSON_SYNTAX")),
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(self.issue("JSON_NON_INTEGER_NUMBER"));
        }
        let token = &self.input[start..self.offset];
        let digits = token
            .len()
            .saturating_sub(usize::from(token.starts_with('-')));
        if digits > MAXIMUM_JSON_INTEGER_DIGITS {
            return Err(JsonIssue::limit(
                "JSON_INTEGER_LIMIT",
                Some(start),
                MAXIMUM_JSON_INTEGER_BITS as usize,
            ));
        }
        let value = BigInt::parse_bytes(token.as_bytes(), 10)
            .ok_or_else(|| JsonIssue::at("JSON_SYNTAX", start))?;
        if value.bits() > MAXIMUM_JSON_INTEGER_BITS {
            return Err(JsonIssue::limit(
                "JSON_INTEGER_LIMIT",
                Some(start),
                MAXIMUM_JSON_INTEGER_BITS as usize,
            ));
        }
        Ok(LibraryValue::Int(value))
    }

    fn parse_string(&mut self) -> Result<String, JsonIssue> {
        if !self.consume_if(b'"') {
            return Err(self.issue("JSON_SYNTAX"));
        }
        let mut output = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.issue("JSON_SYNTAX"));
            };
            match byte {
                b'"' => {
                    self.offset += 1;
                    return Ok(output);
                }
                b'\\' => {
                    self.offset += 1;
                    self.parse_escape(&mut output)?;
                }
                0..=0x1f => return Err(self.issue("JSON_SYNTAX")),
                0x20..=0x7f => {
                    output.push(char::from(byte));
                    self.offset += 1;
                }
                _ => {
                    let character = self.input[self.offset..]
                        .chars()
                        .next()
                        .ok_or_else(|| self.issue("JSON_SYNTAX"))?;
                    output.push(character);
                    self.offset += character.len_utf8();
                }
            }
            if output.len() > MAXIMUM_JSON_STRING_BYTES {
                return Err(JsonIssue::limit(
                    "JSON_STRING_LIMIT",
                    Some(self.offset),
                    MAXIMUM_JSON_STRING_BYTES,
                ));
            }
        }
    }

    fn parse_escape(&mut self, output: &mut String) -> Result<(), JsonIssue> {
        let Some(escape) = self.peek() else {
            return Err(self.issue("JSON_SYNTAX"));
        };
        self.offset += 1;
        let character = match escape {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{0008}',
            b'f' => '\u{000c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => self.parse_unicode_escape()?,
            _ => return Err(self.issue("JSON_SYNTAX")),
        };
        output.push(character);
        Ok(())
    }

    fn parse_unicode_escape(&mut self) -> Result<char, JsonIssue> {
        let first = self.parse_hex_quad()?;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            if !self.consume_if(b'\\') || !self.consume_if(b'u') {
                return Err(self.issue("JSON_SYNTAX"));
            }
            let second = self.parse_hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(self.issue("JSON_SYNTAX"));
            }
            0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err(self.issue("JSON_SYNTAX"));
        } else {
            u32::from(first)
        };
        char::from_u32(scalar).ok_or_else(|| self.issue("JSON_SYNTAX"))
    }

    fn parse_hex_quad(&mut self) -> Result<u16, JsonIssue> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err(self.issue("JSON_SYNTAX"));
            };
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a' + 10),
                b'A'..=b'F' => u16::from(byte - b'A' + 10),
                _ => return Err(self.issue("JSON_SYNTAX")),
            };
            value = value.saturating_mul(16).saturating_add(digit);
            self.offset += 1;
        }
        Ok(value)
    }

    fn consume_literal(&mut self, literal: &[u8]) -> Result<(), JsonIssue> {
        if self
            .bytes
            .get(self.offset..self.offset.saturating_add(literal.len()))
            == Some(literal)
        {
            self.offset += literal.len();
            Ok(())
        } else {
            Err(self.issue("JSON_SYNTAX"))
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.offset += 1;
        }
    }

    fn consume_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    const fn issue(&self, code: &'static str) -> JsonIssue {
        JsonIssue::at(code, self.offset)
    }
}
