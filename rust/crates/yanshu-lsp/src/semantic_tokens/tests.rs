use yanshu_syntax::load_program_source;

use super::{
    MAXIMUM_SEMANTIC_TOKENS, MODIFIER_DECLARATION, MODIFIER_DEFAULT_LIBRARY, MODIFIER_READONLY,
    SemanticToken, TokenType, encode_tokens, semantic_token_candidates, semantic_tokens,
};

const SOURCE: &str = r#"(program
  ; 😀 comment text is not code
  (name semantic)
  (version 4)
  (libraries (text 1))
  (data decision (approved (amount integer)))
  (export-types decision)
  (schema request (object (required "amount" integer)))
  (signature target (fn (integer) integer))
  (def target (fn (value) (if (> value 0) (text/length "😀") value)))
  (signature use (fn (integer) (list integer)))
  (def use (fn (target) (let ((local target)) (list local '(if target)))))
  (export target use approved))"#;

#[test]
fn classifies_forms_types_bindings_and_libraries_without_highlighting_quote_data() {
    let program = load_program_source(SOURCE).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let tokens =
        semantic_token_candidates(&program).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

    assert_token(
        SOURCE,
        &tokens,
        SOURCE
            .find("program")
            .unwrap_or_else(|| panic!("program missing")),
        TokenType::Keyword,
        0,
    );
    assert_token(
        SOURCE,
        &tokens,
        SOURCE
            .find("decision (approved")
            .unwrap_or_else(|| panic!("type missing")),
        TokenType::Type,
        super::MODIFIER_DEFINITION | MODIFIER_READONLY,
    );
    let parameter_declaration = SOURCE
        .rfind("(fn (target)")
        .unwrap_or_else(|| panic!("parameter declaration missing"))
        + "(fn (".len();
    assert_token(
        SOURCE,
        &tokens,
        parameter_declaration,
        TokenType::Parameter,
        MODIFIER_DECLARATION | MODIFIER_READONLY,
    );
    let shadowed_reference = SOURCE
        .rfind("local target")
        .unwrap_or_else(|| panic!("shadowed reference missing"))
        + "local ".len();
    assert_token(
        SOURCE,
        &tokens,
        shadowed_reference,
        TokenType::Parameter,
        MODIFIER_READONLY,
    );
    let library = SOURCE
        .find("text/length")
        .unwrap_or_else(|| panic!("library operation missing"));
    assert_token(
        SOURCE,
        &tokens,
        library,
        TokenType::Function,
        MODIFIER_DEFAULT_LIBRARY,
    );
    let operator = SOURCE
        .find("> value")
        .unwrap_or_else(|| panic!("operator missing"));
    assert_token(
        SOURCE,
        &tokens,
        operator,
        TokenType::Operator,
        MODIFIER_DEFAULT_LIBRARY,
    );
    let quoted_if = SOURCE
        .rfind("if target")
        .unwrap_or_else(|| panic!("quoted if missing"));
    assert!(!tokens.iter().any(|token| {
        token.span.start.offset == quoted_if || token.span.start.offset == quoted_if + "if ".len()
    }));
}

#[test]
fn encodes_sorted_multiline_utf16_tokens_and_stays_read_only() {
    let program = load_program_source(SOURCE).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let tokens =
        semantic_token_candidates(&program).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let data = semantic_tokens(&program).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert_eq!(data.len(), tokens.len() * 5);

    let decoded = decode(&data);
    for (token, (line, start, length, token_type, modifiers)) in tokens.iter().zip(decoded) {
        let expected_start = position_at(SOURCE, token.span.start.offset);
        let expected_end = position_at(SOURCE, token.span.end.offset);
        assert_eq!((line, start), expected_start);
        assert_eq!(length, expected_end.1 - expected_start.1);
        assert_eq!(token_type, token.token_type as u32);
        assert_eq!(modifiers, token.modifiers);
    }
    assert_eq!(program.source, SOURCE);
}

#[test]
fn lexical_binding_classification_wins_over_a_primitive_name() {
    let source = "(program (name shadow) (version 1) (capabilities log) (def run (fn (log) (log log))) (export run))";
    let program = load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let tokens =
        semantic_token_candidates(&program).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let call = source
        .rfind("log log")
        .unwrap_or_else(|| panic!("shadowed primitive call missing"));

    for offset in [call, call + "log ".len()] {
        assert_token(
            source,
            &tokens,
            offset,
            TokenType::Parameter,
            MODIFIER_READONLY,
        );
    }
}

#[test]
fn rejects_results_above_the_bounded_protocol_budget() {
    let span = yanshu_diagnostic::Span {
        start: yanshu_diagnostic::Position {
            offset: 0,
            line: 1,
            column: 1,
        },
        end: yanshu_diagnostic::Position {
            offset: 1,
            line: 1,
            column: 2,
        },
    };
    let token = SemanticToken {
        span,
        token_type: TokenType::Keyword,
        modifiers: 0,
        priority: 0,
    };
    let diagnostic = encode_tokens("x", &vec![token; MAXIMUM_SEMANTIC_TOKENS + 1])
        .err()
        .unwrap_or_else(|| panic!("oversized token set unexpectedly succeeded"));
    assert_eq!(diagnostic.code, "LSP_SEMANTIC_TOKEN_LIMIT");
}

fn assert_token(
    source: &str,
    tokens: &[SemanticToken],
    offset: usize,
    token_type: TokenType,
    modifiers: u32,
) {
    let token = tokens
        .iter()
        .find(|token| token.span.start.offset == offset)
        .unwrap_or_else(|| panic!("semantic token missing at {offset}"));
    assert_eq!(token.token_type, token_type);
    assert_eq!(token.modifiers, modifiers);
    assert!(token.span.end.offset <= source.len());
}

fn decode(data: &[u32]) -> Vec<(u32, u32, u32, u32, u32)> {
    let mut line = 0_u32;
    let mut start = 0_u32;
    data.chunks_exact(5)
        .map(|item| {
            line += item[0];
            start = if item[0] == 0 {
                start + item[1]
            } else {
                item[1]
            };
            (line, start, item[2], item[3], item[4])
        })
        .collect()
}

fn position_at(source: &str, offset: usize) -> (u32, u32) {
    let prefix = source
        .get(..offset)
        .unwrap_or_else(|| panic!("test offset is not a UTF-8 boundary"));
    let line = u32::try_from(prefix.matches('\n').count())
        .unwrap_or_else(|_| panic!("test line overflowed"));
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let character = u32::try_from(prefix[line_start..].encode_utf16().count())
        .unwrap_or_else(|_| panic!("test character overflowed"));
    (line, character)
}
