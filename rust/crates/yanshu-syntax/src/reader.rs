#![forbid(unsafe_code)]

use std::str::FromStr;

use num_bigint::BigInt;
use serde_json::json;
use yanshu_diagnostic::{Diagnostic, Position, Span, YanshuResult};

use crate::{Datum, DatumKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderLimits {
    pub max_source_bytes: usize,
    pub max_token_bytes: usize,
    pub max_string_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for ReaderLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 4 * 1024 * 1024,
            max_token_bytes: 64 * 1024,
            max_string_bytes: 1024 * 1024,
            max_nodes: 10_000,
            max_depth: 128,
        }
    }
}

pub fn read_source(source: &str, limits: ReaderLimits) -> YanshuResult<Datum> {
    if source.len() > limits.max_source_bytes {
        return Err(Diagnostic::new(
            "READ_SOURCE_LIMIT",
            "source exceeds the configured byte limit",
            json!({ "maximum": limits.max_source_bytes, "actual": source.len() }),
        ));
    }
    let mut reader = Reader::new(source, limits);
    reader.skip_ignored();
    if reader.peek().is_none() {
        return Err(Diagnostic::simple("READ_EMPTY", "source document is empty"));
    }

    let root = reader.read_datum(0)?;
    reader.skip_ignored();
    if reader.peek().is_some() {
        // Parse once so malformed trailing input remains a syntax error, matching
        // the reference reader's second read.
        let _trailing = reader.read_datum(0)?;
        return Err(Diagnostic::simple(
            "READ_MULTIPLE_FORMS",
            "source document must contain exactly one top-level form",
        ));
    }
    Ok(root)
}

struct Reader<'source> {
    source: &'source str,
    offset: usize,
    line: usize,
    column: usize,
    node_count: usize,
    limits: ReaderLimits,
}

impl<'source> Reader<'source> {
    fn new(source: &'source str, limits: ReaderLimits) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
            node_count: 0,
            limits,
        }
    }

    fn position(&self) -> Position {
        Position {
            offset: self.offset,
            line: self.line,
            column: self.column,
        }
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.offset..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    fn skip_ignored(&mut self) {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                let _character = self.bump();
            }
            if self.peek() != Some(';') {
                break;
            }
            while let Some(character) = self.bump() {
                if character == '\n' {
                    break;
                }
            }
        }
    }

    fn consume_node(&mut self, depth: usize, start: Position) -> YanshuResult<()> {
        self.node_count += 1;
        if self.node_count > self.limits.max_nodes {
            return Err(Diagnostic::new(
                "READ_NODE_LIMIT",
                "source exceeds the configured node limit",
                json!({ "maxNodes": self.limits.max_nodes }),
            )
            .at(Span {
                start,
                end: self.position(),
            }));
        }
        if depth > self.limits.max_depth {
            return Err(Diagnostic::new(
                "READ_DEPTH_LIMIT",
                "source exceeds the configured nesting limit",
                json!({ "maxDepth": self.limits.max_depth }),
            )
            .at(Span {
                start,
                end: self.position(),
            }));
        }
        Ok(())
    }

    fn read_datum(&mut self, depth: usize) -> YanshuResult<Datum> {
        self.skip_ignored();
        let start = self.position();
        self.consume_node(depth, start)?;
        match self.peek() {
            Some('(') => self.read_list(depth, '(', ')'),
            Some('[') => self.read_list(depth, '[', ']'),
            Some('{') => self.read_list(depth, '{', '}'),
            Some(')') | Some(']') | Some('}') => {
                Err(self.syntax("unexpected closing delimiter", start))
            }
            Some('\'') => self.read_quote(depth),
            Some('"') => self.read_string(),
            Some(_) => self.read_atom(),
            None => Err(self.syntax("unexpected end of source", start)),
        }
    }

    fn read_list(&mut self, depth: usize, open: char, close: char) -> YanshuResult<Datum> {
        let start = self.position();
        let observed = self.bump();
        debug_assert_eq!(observed, Some(open));
        let mut values = Vec::new();
        loop {
            self.skip_ignored();
            match self.peek() {
                Some(character) if character == close => {
                    let _character = self.bump();
                    return Ok(Datum {
                        kind: DatumKind::List(values),
                        span: Span {
                            start,
                            end: self.position(),
                        },
                    });
                }
                Some(')') | Some(']') | Some('}') => {
                    return Err(self.syntax("mismatched closing delimiter", self.position()));
                }
                Some(_) => values.push(self.read_datum(depth + 1)?),
                None => return Err(self.syntax("unterminated list", start)),
            }
        }
    }

    fn read_quote(&mut self, depth: usize) -> YanshuResult<Datum> {
        let start = self.position();
        let _quote = self.bump();
        self.consume_node(depth + 1, start)?;
        let symbol = Datum {
            kind: DatumKind::Symbol("quote".to_owned()),
            span: Span {
                start,
                end: self.position(),
            },
        };
        let quoted = self.read_datum(depth + 1)?;
        Ok(Datum {
            span: Span {
                start,
                end: quoted.span.end,
            },
            kind: DatumKind::List(vec![symbol, quoted]),
        })
    }

    fn read_string(&mut self) -> YanshuResult<Datum> {
        let start = self.position();
        let _opening_quote = self.bump();
        let mut value = String::new();
        loop {
            match self.bump() {
                Some('"') => {
                    return Ok(Datum {
                        kind: DatumKind::String(value),
                        span: Span {
                            start,
                            end: self.position(),
                        },
                    });
                }
                Some('\\') => {
                    let escaped = match self.bump() {
                        Some('a') => '\u{0007}',
                        Some('b') => '\u{0008}',
                        Some('t') => '\t',
                        Some('n') => '\n',
                        Some('v') => '\u{000b}',
                        Some('f') => '\u{000c}',
                        Some('r') => '\r',
                        Some('e') => '\u{001b}',
                        Some('"') => '"',
                        Some('\'') => '\'',
                        Some('\\') => '\\',
                        Some(character) => {
                            return Err(self.syntax(
                                &format!("unsupported string escape: \\{character}"),
                                start,
                            ));
                        }
                        None => return Err(self.syntax("unterminated string escape", start)),
                    };
                    value.push(escaped);
                    self.check_string_limit(value.len(), start)?;
                }
                Some(character) => {
                    value.push(character);
                    self.check_string_limit(value.len(), start)?;
                }
                None => return Err(self.syntax("unterminated string", start)),
            }
        }
    }

    fn read_atom(&mut self) -> YanshuResult<Datum> {
        let start = self.position();
        let token_start = self.offset;
        while self.peek().is_some_and(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ';'
                )
        }) {
            let _character = self.bump();
            if self.offset.saturating_sub(token_start) > self.limits.max_token_bytes {
                return Err(Diagnostic::new(
                    "READ_TOKEN_LIMIT",
                    "source token exceeds the configured byte limit",
                    json!({ "maximum": self.limits.max_token_bytes }),
                )
                .at(Span {
                    start,
                    end: self.position(),
                }));
            }
        }
        let token = self
            .source
            .get(token_start..self.offset)
            .ok_or_else(|| self.syntax("source token is not on a UTF-8 boundary", start))?;
        let kind = match token {
            "#t" | "#true" => DatumKind::Bool(true),
            "#f" | "#false" => DatumKind::Bool(false),
            "." => return Err(self.syntax("dot is not a supported datum", start)),
            _ if decimal_integer(token) => {
                let integer = BigInt::from_str(token)
                    .map_err(|_| self.syntax("invalid exact integer", start))?;
                DatumKind::Integer(integer)
            }
            _ if token.starts_with('#') => {
                return Err(Diagnostic::new(
                    "READ_UNSUPPORTED_DATUM",
                    "source contains an unsupported datum",
                    json!({ "datum": token }),
                )
                .at(Span {
                    start,
                    end: self.position(),
                }));
            }
            _ => DatumKind::Symbol(token.to_owned()),
        };
        Ok(Datum {
            kind,
            span: Span {
                start,
                end: self.position(),
            },
        })
    }

    fn syntax(&self, message: &str, start: Position) -> Diagnostic {
        Diagnostic::simple("READ_SYNTAX", message).at(Span {
            start,
            end: self.position(),
        })
    }

    fn check_string_limit(&self, actual: usize, start: Position) -> YanshuResult<()> {
        if actual <= self.limits.max_string_bytes {
            return Ok(());
        }
        Err(Diagnostic::new(
            "READ_STRING_LIMIT",
            "string literal exceeds the configured byte limit",
            json!({ "maximum": self.limits.max_string_bytes }),
        )
        .at(Span {
            start,
            end: self.position(),
        }))
    }
}

fn decimal_integer(token: &str) -> bool {
    let digits = token
        .strip_prefix('+')
        .or_else(|| token.strip_prefix('-'))
        .unwrap_or(token);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use yanshu_diagnostic::{Diagnostic, YanshuResult};

    use super::{ReaderLimits, read_source};
    use crate::DatumKind;

    fn require_error<T>(result: YanshuResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    #[test]
    fn reads_unicode_comments_quotes_and_arbitrary_integers() {
        let datum = read_source(
            "; comment\n('语言 9223372036854775808 \"ok\\n\")",
            ReaderLimits::default(),
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let DatumKind::List(values) = datum.kind else {
            panic!("expected list")
        };
        assert_eq!(values.len(), 3);
        assert_eq!(values[0].display(), "(quote 语言)");
        assert_eq!(values[1].display(), "9223372036854775808");
        assert_eq!(values[2].display(), "\"ok\\n\"");
    }

    #[test]
    fn reports_stable_document_boundaries() {
        let empty = require_error(read_source(" ; only comment", ReaderLimits::default()));
        assert_eq!(empty.code, "READ_EMPTY");

        let multiple = require_error(read_source("(program) (program)", ReaderLimits::default()));
        assert_eq!(multiple.code, "READ_MULTIPLE_FORMS");
    }

    #[test]
    fn enforces_node_and_depth_limits() {
        let nodes = require_error(read_source(
            "(a b)",
            ReaderLimits {
                max_nodes: 2,
                max_depth: 10,
                ..ReaderLimits::default()
            },
        ));
        assert_eq!(nodes.code, "READ_NODE_LIMIT");

        let depth = require_error(read_source(
            "((a))",
            ReaderLimits {
                max_nodes: 10,
                max_depth: 1,
                ..ReaderLimits::default()
            },
        ));
        assert_eq!(depth.code, "READ_DEPTH_LIMIT");
    }

    #[test]
    fn rejects_oversized_sources_tokens_and_strings_before_parsing_them() {
        let source = require_error(read_source(
            "(abcd)",
            ReaderLimits {
                max_source_bytes: 5,
                ..ReaderLimits::default()
            },
        ));
        assert_eq!(source.code, "READ_SOURCE_LIMIT");

        let token = require_error(read_source(
            "abcd",
            ReaderLimits {
                max_token_bytes: 3,
                ..ReaderLimits::default()
            },
        ));
        assert_eq!(token.code, "READ_TOKEN_LIMIT");

        let string = require_error(read_source(
            "\"four\"",
            ReaderLimits {
                max_string_bytes: 3,
                ..ReaderLimits::default()
            },
        ));
        assert_eq!(string.code, "READ_STRING_LIMIT");
    }
}
