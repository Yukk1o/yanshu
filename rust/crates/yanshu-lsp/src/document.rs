use std::collections::BTreeMap;

use serde_json::{Value, json};
use yanshu_analysis::{AnalysisReport, analyze_program, render_rust_review};
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_format::{FormatOptions, format_source};
use yanshu_syntax::{Program, ReaderLimits, load_program_source, symbol_index};

use crate::hover::hover_at;

const MAXIMUM_OPEN_DOCUMENTS: usize = 32;
const MAXIMUM_TOTAL_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAXIMUM_URI_BYTES: usize = 4 * 1024;
const MAXIMUM_REFERENCE_LOCATIONS: usize = 1024;
const MAXIMUM_LOCATION_JSON_OVERHEAD_BYTES: usize = 1024;
pub(crate) const MAXIMUM_REVIEW_SOURCE_BYTES: usize = 512 * 1024;
pub(crate) const MAXIMUM_REVIEW_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const REVIEW_RENDERER: &str = "rust-readonly-v3";
pub(crate) const REVIEW_LANGUAGE_ID: &str = "rust";

// A JSON string byte can expand to a six-byte `\u00xx` escape. Keep the
// complete worst-case Location[] below the protocol's outbound body limit.
const _: () = assert!(
    MAXIMUM_REFERENCE_LOCATIONS * (MAXIMUM_URI_BYTES * 6 + MAXIMUM_LOCATION_JSON_OVERHEAD_BYTES)
        < crate::protocol::MAXIMUM_LSP_MESSAGE_BYTES
);

// A serialized JSON string byte can expand to a six-byte escape. Keep the
// complete review response below the outbound protocol body limit.
const _: () = assert!(
    MAXIMUM_REVIEW_TEXT_BYTES * 6 + MAXIMUM_LOCATION_JSON_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_LSP_MESSAGE_BYTES
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenDocument {
    pub(crate) uri: String,
    pub(crate) version: i64,
    pub(crate) source: String,
}

#[derive(Default)]
pub(crate) struct DocumentStore {
    documents: BTreeMap<String, OpenDocument>,
    total_source_bytes: usize,
}

impl DocumentStore {
    pub(crate) fn open(&mut self, uri: &str, version: i64, source: &str) -> YanshuResult<()> {
        validate_uri(uri)?;
        validate_source(source)?;
        if self.documents.contains_key(uri) {
            return Err(Diagnostic::simple(
                "LSP_DOCUMENT_OPEN",
                "LSP document is already open",
            ));
        }
        if !self.documents.contains_key(uri) && self.documents.len() >= MAXIMUM_OPEN_DOCUMENTS {
            return Err(Diagnostic::new(
                "LSP_DOCUMENT_LIMIT",
                "LSP server has reached its open document limit",
                json!({ "maximum": MAXIMUM_OPEN_DOCUMENTS }),
            ));
        }
        self.replace(uri, version, source)
    }

    pub(crate) fn change(&mut self, uri: &str, version: i64, source: &str) -> YanshuResult<()> {
        validate_source(source)?;
        let previous = self.documents.get(uri).ok_or_else(|| {
            Diagnostic::simple(
                "LSP_DOCUMENT_UNKNOWN",
                "LSP change refers to a document that is not open",
            )
        })?;
        if version <= previous.version {
            return Err(Diagnostic::new(
                "LSP_DOCUMENT_VERSION",
                "LSP document versions must increase monotonically",
                json!({ "previous": previous.version, "actual": version }),
            ));
        }
        self.replace(uri, version, source)
    }

    fn replace(&mut self, uri: &str, version: i64, source: &str) -> YanshuResult<()> {
        let previous_bytes = self
            .documents
            .get(uri)
            .map_or(0, |document| document.source.len());
        let next_total = self
            .total_source_bytes
            .saturating_sub(previous_bytes)
            .checked_add(source.len())
            .ok_or_else(|| {
                Diagnostic::simple(
                    "LSP_DOCUMENT_LIMIT",
                    "LSP document byte accounting overflowed",
                )
            })?;
        if next_total > MAXIMUM_TOTAL_SOURCE_BYTES {
            return Err(Diagnostic::new(
                "LSP_DOCUMENT_LIMIT",
                "LSP open documents exceed the total source byte limit",
                json!({ "actual": next_total, "maximum": MAXIMUM_TOTAL_SOURCE_BYTES }),
            ));
        }
        self.documents.insert(
            uri.to_owned(),
            OpenDocument {
                uri: uri.to_owned(),
                version,
                source: source.to_owned(),
            },
        );
        self.total_source_bytes = next_total;
        Ok(())
    }

    pub(crate) fn close(&mut self, uri: &str) -> bool {
        let Some(document) = self.documents.remove(uri) else {
            return false;
        };
        self.total_source_bytes = self
            .total_source_bytes
            .saturating_sub(document.source.len());
        true
    }

    pub(crate) fn get(&self, uri: &str) -> Option<&OpenDocument> {
        self.documents.get(uri)
    }
}

impl OpenDocument {
    pub(crate) fn diagnostics(&self) -> Vec<Value> {
        let program = match load_program_source(&self.source) {
            Ok(program) => program,
            Err(diagnostic) => return vec![lsp_diagnostic(&self.source, &diagnostic)],
        };
        if program.version.to_string() != "4" {
            return Vec::new();
        }
        match analyze_program(&program) {
            Ok(_) => Vec::new(),
            Err(diagnostic) => vec![lsp_diagnostic(&self.source, &diagnostic)],
        }
    }

    pub(crate) fn hover(&self, line: u64, character: u64) -> Option<Value> {
        let offset = offset_from_lsp(&self.source, line, character)?;
        let program = load_program_source(&self.source).ok()?;
        let analysis = analyze_for_tools(&program);
        let hover = hover_at(&program, analysis.as_ref(), offset)?;
        Some(json!({
            "contents": { "kind": "plaintext", "value": hover.text },
            "range": span_range(&self.source, hover.span),
        }))
    }

    pub(crate) fn definition(&self, line: u64, character: u64) -> Option<Value> {
        let offset = offset_from_lsp(&self.source, line, character)?;
        let program = load_program_source(&self.source).ok()?;
        let target = symbol_index(&program).ok()?.definition_at(offset)?;
        Some(json!({
            "uri": self.uri,
            "range": span_range(&self.source, target),
        }))
    }

    pub(crate) fn references(
        &self,
        line: u64,
        character: u64,
        include_declaration: bool,
    ) -> YanshuResult<Option<Vec<Value>>> {
        let Some(offset) = offset_from_lsp(&self.source, line, character) else {
            return Ok(None);
        };
        let Ok(program) = load_program_source(&self.source) else {
            return Ok(None);
        };
        let Ok(index) = symbol_index(&program) else {
            return Ok(None);
        };
        let Some(spans) = index.references_at(offset, include_declaration) else {
            return Ok(None);
        };
        if spans.len() > MAXIMUM_REFERENCE_LOCATIONS {
            return Err(Diagnostic::new(
                "LSP_REFERENCE_LIMIT",
                "LSP reference result exceeds the configured location limit",
                json!({
                    "actual": spans.len(),
                    "maximum": MAXIMUM_REFERENCE_LOCATIONS,
                }),
            ));
        }
        Ok(locations_for_sorted_spans(&self.uri, &self.source, &spans))
    }

    pub(crate) fn review(&self, expected_version: i64) -> YanshuResult<Value> {
        self.review_with_limits(
            expected_version,
            MAXIMUM_REVIEW_SOURCE_BYTES,
            MAXIMUM_REVIEW_TEXT_BYTES,
        )
    }

    fn review_with_limits(
        &self,
        expected_version: i64,
        maximum_source_bytes: usize,
        maximum_text_bytes: usize,
    ) -> YanshuResult<Value> {
        if self.version != expected_version {
            return Err(Diagnostic::new(
                "LSP_REVIEW_VERSION",
                "review preview requires the current open document version",
                json!({ "expected": expected_version, "actual": self.version }),
            ));
        }
        if self.source.len() > maximum_source_bytes {
            return Err(Diagnostic::new(
                "LSP_REVIEW_SOURCE_LIMIT",
                "review preview source exceeds the configured byte limit",
                json!({
                    "actual": self.source.len(),
                    "maximum": maximum_source_bytes,
                }),
            ));
        }
        let program = load_program_source(&self.source)?;
        let analysis = analyze_program(&program)?;
        let review = render_rust_review(&program, &analysis);
        if review.renderer != REVIEW_RENDERER || review.editable {
            return Err(Diagnostic::simple(
                "LSP_REVIEW_CONTRACT",
                "review renderer violated the read-only protocol contract",
            ));
        }
        if review.text.len() > maximum_text_bytes {
            return Err(Diagnostic::new(
                "LSP_REVIEW_TEXT_LIMIT",
                "review preview text exceeds the configured byte limit",
                json!({
                    "actual": review.text.len(),
                    "maximum": maximum_text_bytes,
                }),
            ));
        }
        Ok(json!({
            "sourceVersion": self.version,
            "renderer": review.renderer,
            "editable": review.editable,
            "languageId": REVIEW_LANGUAGE_ID,
            "text": review.text,
        }))
    }

    pub(crate) fn formatting_edits(&self) -> YanshuResult<Vec<Value>> {
        let formatted = format_source(&self.source, FormatOptions::default())?;
        if !formatted.changed {
            return Ok(Vec::new());
        }
        Ok(vec![json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": lsp_position(&self.source, self.source.len()),
            },
            "newText": formatted.source,
        })])
    }
}

fn validate_uri(uri: &str) -> YanshuResult<()> {
    if uri.is_empty() || uri.len() > MAXIMUM_URI_BYTES {
        return Err(Diagnostic::new(
            "LSP_URI_LIMIT",
            "LSP document URI is empty or exceeds the configured byte limit",
            json!({ "actual": uri.len(), "maximum": MAXIMUM_URI_BYTES }),
        ));
    }
    Ok(())
}

fn validate_source(source: &str) -> YanshuResult<()> {
    let maximum = ReaderLimits::default().max_source_bytes;
    if source.len() > maximum {
        return Err(Diagnostic::new(
            "LSP_SOURCE_LIMIT",
            "LSP document exceeds the language source byte limit",
            json!({ "actual": source.len(), "maximum": maximum }),
        ));
    }
    Ok(())
}

fn analyze_for_tools(program: &Program) -> Option<AnalysisReport> {
    (program.version.to_string() == "4")
        .then(|| analyze_program(program).ok())
        .flatten()
}

fn lsp_diagnostic(source: &str, diagnostic: &Diagnostic) -> Value {
    let range = diagnostic.span.as_deref().map_or_else(
        || {
            let end_character = source.chars().next().map_or(0, char::len_utf16);
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": end_character },
            })
        },
        |span| span_range(source, *span),
    );
    json!({
        "range": range,
        "severity": 1,
        "code": diagnostic.code,
        "source": "yanshu",
        "message": diagnostic.message.as_ref(),
        "data": diagnostic.details.as_ref(),
    })
}

fn span_range(source: &str, span: Span) -> Value {
    json!({
        "start": lsp_position(source, span.start.offset),
        "end": lsp_position(source, span.end.offset),
    })
}

fn lsp_position(source: &str, offset: usize) -> Value {
    let bounded = offset.min(source.len());
    let prefix = source.get(..bounded).unwrap_or_default();
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let character = prefix
        .get(line_start..)
        .unwrap_or_default()
        .encode_utf16()
        .count();
    json!({ "line": line, "character": character })
}

fn offset_from_lsp(source: &str, target_line: u64, target_character: u64) -> Option<usize> {
    let target_line = usize::try_from(target_line).ok()?;
    let target_character = usize::try_from(target_character).ok()?;
    let mut line_start = 0_usize;
    for _ in 0..target_line {
        let relative = source.get(line_start..)?.find('\n')?;
        line_start = line_start.checked_add(relative)?.checked_add(1)?;
    }
    let remainder = source.get(line_start..)?;
    let mut line_end = remainder.find('\n').unwrap_or(remainder.len());
    if remainder.get(..line_end)?.ends_with('\r') {
        line_end = line_end.saturating_sub(1);
    }
    let line = remainder.get(..line_end)?;
    let mut utf16 = 0_usize;
    for (byte, character) in line.char_indices() {
        if utf16 == target_character {
            return line_start.checked_add(byte);
        }
        utf16 = utf16.checked_add(character.len_utf16())?;
        if utf16 > target_character {
            return None;
        }
    }
    (utf16 == target_character).then(|| line_start + line.len())
}

fn locations_for_sorted_spans(uri: &str, source: &str, spans: &[Span]) -> Option<Vec<Value>> {
    let mut cursor = PositionCursor::new(source);
    let mut locations = Vec::with_capacity(spans.len());
    for span in spans {
        let start = cursor.advance_to(span.start.offset)?;
        let end = cursor.advance_to(span.end.offset)?;
        locations.push(json!({
            "uri": uri,
            "range": {
                "start": { "line": start.0, "character": start.1 },
                "end": { "line": end.0, "character": end.1 },
            },
        }));
    }
    Some(locations)
}

struct PositionCursor<'source> {
    source: &'source str,
    offset: usize,
    line: usize,
    character: usize,
}

impl<'source> PositionCursor<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            offset: 0,
            line: 0,
            character: 0,
        }
    }

    fn advance_to(&mut self, target: usize) -> Option<(usize, usize)> {
        if target < self.offset || target > self.source.len() {
            return None;
        }
        for character in self.source.get(self.offset..target)?.chars() {
            if character == '\n' {
                self.line = self.line.checked_add(1)?;
                self.character = 0;
            } else {
                self.character = self.character.checked_add(character.len_utf16())?;
            }
        }
        self.offset = target;
        Some((self.line, self.character))
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenDocument, lsp_position, offset_from_lsp};

    fn document(source: &str) -> OpenDocument {
        OpenDocument {
            uri: "file:///policy.yan".to_owned(),
            version: 1,
            source: source.to_owned(),
        }
    }

    fn character_at(source: &str, offset: usize) -> u64 {
        u64::try_from(source[..offset].encode_utf16().count())
            .unwrap_or_else(|_| panic!("test character offset overflowed"))
    }

    fn definition_start(document: &OpenDocument, source: &str, offset: usize) -> u64 {
        document
            .definition(0, character_at(source, offset))
            .and_then(|location| location["range"]["start"]["character"].as_u64())
            .unwrap_or_else(|| panic!("definition location missing"))
    }

    fn reference_starts(
        document: &OpenDocument,
        source: &str,
        offset: usize,
        include_declaration: bool,
    ) -> Vec<u64> {
        document
            .references(0, character_at(source, offset), include_declaration)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("reference locations missing"))
            .iter()
            .map(|location| {
                location["range"]["start"]["character"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("reference start missing"))
            })
            .collect()
    }

    #[test]
    fn utf16_positions_do_not_split_surrogate_pairs() {
        assert_eq!(offset_from_lsp("a😀b", 0, 1), Some(1));
        assert_eq!(offset_from_lsp("a😀b", 0, 2), None);
        assert_eq!(offset_from_lsp("a😀b", 0, 3), Some(5));
    }

    #[test]
    fn hover_returns_only_the_exact_utf16_token_range() {
        let source = "(program\n  ; 😀 stays outside the token range\n  (name hover-range)\n  (version 4)\n  (signature run (fn (integer) integer))\n  (def run (fn (value)\n    (cond\n      ((> value 0) value)\n      (else 0))))\n  (export run))";
        let document = document(source);
        let offset = source
            .find("cond\n")
            .unwrap_or_else(|| panic!("cond token missing"));
        let start = lsp_position(source, offset);
        let end = lsp_position(source, offset + "cond".len());
        let hover = document
            .hover(
                start["line"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("hover line missing")),
                start["character"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("hover character missing")),
            )
            .unwrap_or_else(|| panic!("cond hover missing"));
        assert_eq!(hover["range"]["start"], start);
        assert_eq!(hover["range"]["end"], end);
        assert!(
            hover["contents"]["value"]
                .as_str()
                .is_some_and(|text| text.contains("short-circuit special form"))
        );

        assert_eq!(
            document.hover(
                end["line"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("newline line missing")),
                end["character"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("newline character missing")),
            ),
            None
        );
    }

    #[test]
    fn definition_resolves_a_parameter_that_shadows_a_global() {
        let source = "(program (name nav) (version 4) (signature target (fn (integer) integer)) (def target (fn (x) x)) (signature use (fn (integer) integer)) (def use (fn (target) target)) (export target use))";
        let document = document(source);
        let declaration = source
            .rfind("(fn (target)")
            .unwrap_or_else(|| panic!("local target declaration missing"))
            + "(fn (".len();
        let local_offset = source
            .rfind("target))")
            .unwrap_or_else(|| panic!("local target fixture missing"));
        assert_eq!(
            definition_start(&document, source, local_offset),
            character_at(source, declaration),
        );
    }

    #[test]
    fn definition_respects_sequential_let_scope_and_shadowing() {
        let source = "(program (name lets) (version 1) (def use (fn (outer) (let ((value outer) (outer value)) (+ outer value)))) (export use))";
        let document = document(source);
        let parameter = source
            .find("(fn (outer)")
            .unwrap_or_else(|| panic!("parameter declaration missing"))
            + "(fn (".len();
        let first_binding = source
            .find("((value outer)")
            .unwrap_or_else(|| panic!("first let declaration missing"))
            + "((".len();
        let second_binding = source
            .find("(outer value)")
            .unwrap_or_else(|| panic!("second let declaration missing"))
            + '('.len_utf8();
        let parameter_reference = source
            .find("value outer)")
            .unwrap_or_else(|| panic!("parameter reference missing"))
            + "value ".len();
        let first_binding_reference = source
            .find("(outer value)")
            .unwrap_or_else(|| panic!("first let reference missing"))
            + "(outer ".len();
        let body = source
            .find("(+ outer value)")
            .unwrap_or_else(|| panic!("let body missing"));
        let second_binding_reference = body + "(+ ".len();
        let body_first_binding_reference = body + "(+ outer ".len();

        for (reference, declaration) in [
            (parameter_reference, parameter),
            (first_binding_reference, first_binding),
            (second_binding_reference, second_binding),
            (body_first_binding_reference, first_binding),
        ] {
            assert_eq!(
                definition_start(&document, source, reference),
                character_at(source, declaration),
            );
        }
    }

    #[test]
    fn definition_resolves_pattern_bindings_only_inside_their_arm() {
        let source = "(program (name patterns) (version 3) (data decision (approved amount)) (def inspect (fn (decision) (match decision ((approved amount) amount) (_ decision)))) (export inspect approved))";
        let document = document(source);
        let pattern = source
            .find("(approved amount) amount")
            .unwrap_or_else(|| panic!("pattern fixture missing"))
            + "(approved ".len();
        let pattern_reference = source
            .find("(approved amount) amount")
            .unwrap_or_else(|| panic!("pattern reference missing"))
            + "(approved amount) ".len();
        let parameter = source
            .find("(fn (decision)")
            .unwrap_or_else(|| panic!("pattern parameter missing"))
            + "(fn (".len();
        let default_reference = source
            .rfind("_ decision")
            .unwrap_or_else(|| panic!("default arm reference missing"))
            + "_ ".len();

        assert_eq!(
            definition_start(&document, source, pattern_reference),
            character_at(source, pattern),
        );
        assert_eq!(
            definition_start(&document, source, default_reference),
            character_at(source, parameter),
        );
    }

    #[test]
    fn references_separate_global_definitions_from_local_shadowing() {
        let source = "(program (name refs) (version 1) (def target (fn (value) value)) (def shadow (fn (target) (list target target))) (def call (fn (value) (target value))) (export target shadow call))";
        let document = document(source);
        let global_declaration = source
            .find("(def target")
            .unwrap_or_else(|| panic!("global target declaration missing"))
            + "(def ".len();
        let global_reference = source
            .rfind("(target value)")
            .unwrap_or_else(|| panic!("global target reference missing"))
            + '('.len_utf8();
        let export_reference = source
            .rfind("(export target")
            .unwrap_or_else(|| panic!("global target export missing"))
            + "(export ".len();
        assert_eq!(
            reference_starts(&document, source, global_reference, false),
            vec![
                character_at(source, global_reference),
                character_at(source, export_reference),
            ]
        );
        assert_eq!(
            reference_starts(&document, source, global_reference, true),
            vec![
                character_at(source, global_declaration),
                character_at(source, global_reference),
                character_at(source, export_reference),
            ]
        );

        let local_declaration = source
            .find("(fn (target)")
            .unwrap_or_else(|| panic!("local target declaration missing"))
            + "(fn (".len();
        let first_local_reference = source
            .find("(list target target)")
            .unwrap_or_else(|| panic!("local target references missing"))
            + "(list ".len();
        let second_local_reference = first_local_reference + "target ".len();
        assert_eq!(
            reference_starts(&document, source, first_local_reference, true),
            vec![
                character_at(source, local_declaration),
                character_at(source, first_local_reference),
                character_at(source, second_local_reference),
            ]
        );
    }

    #[test]
    fn references_respect_sequential_let_and_pattern_arm_scopes() {
        let let_source = "(program (name lets) (version 1) (def use (fn (outer) (let ((value outer) (outer value)) (list outer value)))) (export use))";
        let let_document = document(let_source);
        let parameter_declaration = let_source
            .find("(fn (outer)")
            .unwrap_or_else(|| panic!("outer parameter missing"))
            + "(fn (".len();
        let parameter_reference = let_source
            .find("((value outer)")
            .unwrap_or_else(|| panic!("outer parameter reference missing"))
            + "((value ".len();
        assert_eq!(
            reference_starts(&let_document, let_source, parameter_reference, true,),
            vec![
                character_at(let_source, parameter_declaration),
                character_at(let_source, parameter_reference),
            ]
        );

        let pattern_source = "(program (name patterns) (version 3) (data decision (approved amount) (rejected reason)) (def inspect (fn (decision) (match decision ((approved value) value) ((rejected value) value) (_ decision)))) (export inspect approved rejected))";
        let pattern_document = document(pattern_source);
        let first_pattern = pattern_source
            .find("(approved value) value")
            .unwrap_or_else(|| panic!("first pattern arm missing"));
        let first_declaration = first_pattern + "(approved ".len();
        let first_reference = first_pattern + "(approved value) ".len();
        let second_pattern = pattern_source
            .find("(rejected value) value")
            .unwrap_or_else(|| panic!("second pattern arm missing"));
        let second_declaration = second_pattern + "(rejected ".len();
        let second_reference = second_pattern + "(rejected value) ".len();
        assert_eq!(
            reference_starts(&pattern_document, pattern_source, first_reference, true,),
            vec![
                character_at(pattern_source, first_declaration),
                character_at(pattern_source, first_reference),
            ]
        );
        assert_eq!(
            reference_starts(&pattern_document, pattern_source, second_reference, true,),
            vec![
                character_at(pattern_source, second_declaration),
                character_at(pattern_source, second_reference),
            ]
        );
    }

    #[test]
    fn references_stream_multiline_utf16_positions_in_source_order() {
        let source = "(program\n  ; 😀 does not shift later UTF-16 lines\n  (name unicode)\n  (version 1)\n  (def use\n    (fn (value)\n      (list value value)))\n  (export use))";
        let document = document(source);
        let selected = source
            .rfind("value)))")
            .unwrap_or_else(|| panic!("selected value reference missing"));
        let position = lsp_position(source, selected);
        let locations = document
            .references(
                position["line"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("selected line missing")),
                position["character"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("selected character missing")),
                true,
            )
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("multiline references missing"));
        assert_eq!(
            locations
                .iter()
                .map(|location| location["range"]["start"]["line"].as_u64())
                .collect::<Vec<_>>(),
            vec![Some(5), Some(6), Some(6)]
        );
        assert!(locations.windows(2).all(|pair| {
            let left = &pair[0]["range"]["start"];
            let right = &pair[1]["range"]["start"];
            (left["line"].as_u64(), left["character"].as_u64())
                < (right["line"].as_u64(), right["character"].as_u64())
        }));
    }

    #[test]
    fn references_fail_before_building_an_unbounded_location_response() {
        let mut source =
            String::from("(program (name bounded) (version 1) (def use (fn (value) (list");
        for _ in 0..=super::MAXIMUM_REFERENCE_LOCATIONS {
            source.push_str(" value");
        }
        source.push_str("))) (export use))");
        let document = document(&source);
        let reference = source
            .rfind("value")
            .unwrap_or_else(|| panic!("bounded reference fixture missing"));
        let diagnostic = document
            .references(0, character_at(&source, reference), false)
            .err()
            .unwrap_or_else(|| panic!("oversized reference result unexpectedly succeeded"));
        assert_eq!(diagnostic.code, "LSP_REFERENCE_LIMIT");
        assert_eq!(
            diagnostic.details.as_ref(),
            &serde_json::json!({
                "actual": super::MAXIMUM_REFERENCE_LOCATIONS + 1,
                "maximum": super::MAXIMUM_REFERENCE_LOCATIONS,
            })
        );
    }

    #[test]
    fn review_uses_the_versioned_snapshot_and_enforces_output_limits() {
        let source = "(program (name preview) (version 4) (capabilities log) (signature use (fn (integer) integer)) (def use (fn (value) (do (log value) value))) (export use))";
        let document = document(source);
        let review = document
            .review(1)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(review["sourceVersion"], 1);
        assert_eq!(review["renderer"], super::REVIEW_RENDERER);
        assert_eq!(review["editable"], false);
        assert_eq!(review["languageId"], super::REVIEW_LANGUAGE_ID);
        assert!(
            review["text"]
                .as_str()
                .is_some_and(|text| text.contains("READ ONLY") && text.contains("log!(value)"))
        );
        assert_eq!(document.source, source);

        let stale = document
            .review(0)
            .err()
            .unwrap_or_else(|| panic!("stale review version unexpectedly succeeded"));
        assert_eq!(stale.code, "LSP_REVIEW_VERSION");

        let oversized_source = document
            .review_with_limits(1, source.len() - 1, usize::MAX)
            .err()
            .unwrap_or_else(|| panic!("oversized review source unexpectedly succeeded"));
        assert_eq!(oversized_source.code, "LSP_REVIEW_SOURCE_LIMIT");

        let oversized_text = document
            .review_with_limits(1, usize::MAX, 1)
            .err()
            .unwrap_or_else(|| panic!("oversized review text unexpectedly succeeded"));
        assert_eq!(oversized_text.code, "LSP_REVIEW_TEXT_LIMIT");
    }

    #[test]
    fn diagnostics_and_formatting_are_read_only() {
        let invalid = document("(program (name broken) (version 4)");
        assert_eq!(invalid.diagnostics()[0]["code"], "READ_SYNTAX");

        let source = "(program (name fmt) (version 1) (def value (fn () 1)) (export value))";
        let valid = document(source);
        let edits = valid
            .formatting_edits()
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(edits.len(), 1);
        assert_eq!(valid.source, source);
    }
}
