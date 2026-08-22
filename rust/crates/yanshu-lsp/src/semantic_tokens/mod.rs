use serde_json::json;
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_syntax::Program;

mod classify;
mod encode;

use classify::semantic_token_candidates;
use encode::encode_tokens;

pub(crate) const SEMANTIC_TOKEN_TYPES: [&str; 9] = [
    "namespace",
    "type",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "function",
    "keyword",
    "operator",
];
pub(crate) const SEMANTIC_TOKEN_MODIFIERS: [&str; 4] =
    ["declaration", "definition", "readonly", "defaultLibrary"];
pub(crate) const MAXIMUM_SEMANTIC_TOKENS: usize = 10_000;

const SEMANTIC_TOKEN_INTEGER_COUNT: usize = 5;
const MAXIMUM_SERIALIZED_U32_BYTES: usize = 11;
const MAXIMUM_SEMANTIC_TOKEN_JSON_OVERHEAD_BYTES: usize = 1024;

const _: () = assert!(SEMANTIC_TOKEN_TYPES.len() < 65_536);
const _: () = assert!(SEMANTIC_TOKEN_MODIFIERS.len() <= u32::BITS as usize);
const _: () = assert!(
    MAXIMUM_SEMANTIC_TOKENS * SEMANTIC_TOKEN_INTEGER_COUNT * MAXIMUM_SERIALIZED_U32_BYTES
        + MAXIMUM_SEMANTIC_TOKEN_JSON_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_LSP_MESSAGE_BYTES
);

const MODIFIER_DECLARATION: u32 = 1 << 0;
const MODIFIER_DEFINITION: u32 = 1 << 1;
const MODIFIER_READONLY: u32 = 1 << 2;
const MODIFIER_DEFAULT_LIBRARY: u32 = 1 << 3;

const PRIORITY_LIBRARY: u8 = 10;
const PRIORITY_STRUCTURE: u8 = 20;
const PRIORITY_BINDING: u8 = 30;

type SpanKey = (usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum TokenType {
    Namespace = 0,
    Type = 1,
    Parameter = 2,
    Variable = 3,
    Property = 4,
    EnumMember = 5,
    Function = 6,
    Keyword = 7,
    Operator = 8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticToken {
    span: Span,
    token_type: TokenType,
    modifiers: u32,
    priority: u8,
}

pub(crate) fn semantic_tokens(program: &Program) -> YanshuResult<Vec<u32>> {
    let tokens = semantic_token_candidates(program)?;
    encode_tokens(&program.source, &tokens)
}

fn span_key(span: Span) -> SpanKey {
    (span.start.offset, span.end.offset)
}

fn span_mismatch() -> Diagnostic {
    Diagnostic::simple(
        "LSP_SEMANTIC_TOKEN_SPAN",
        "semantic token spans do not match the parsed document snapshot",
    )
}

fn token_limit(actual: usize) -> Diagnostic {
    Diagnostic::new(
        "LSP_SEMANTIC_TOKEN_LIMIT",
        "semantic token result exceeds the configured token limit",
        json!({ "actual": actual, "maximum": MAXIMUM_SEMANTIC_TOKENS }),
    )
}

#[cfg(test)]
mod tests;
