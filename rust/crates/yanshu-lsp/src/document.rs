use std::collections::BTreeMap;

use serde_json::{Value, json};
use yanshu_analysis::{AnalysisReport, analyze_program};
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_format::{FormatOptions, format_source};
use yanshu_syntax::{
    Datum, DatumKind, Expression, ExpressionKind, Program, ReaderLimits, expression_nodes,
    load_program_source, local_symbol_index, read_source,
};

const MAXIMUM_OPEN_DOCUMENTS: usize = 32;
const MAXIMUM_TOTAL_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAXIMUM_URI_BYTES: usize = 4 * 1024;

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
        let analysis = analyze_for_tools(&program)?;
        let definition = program
            .definitions
            .iter()
            .find(|definition| contains_offset(definition.expression.span, offset))?;
        let definition_analysis = analysis
            .definitions
            .iter()
            .find(|candidate| candidate.name == definition.name)?;
        let node = expression_nodes(&program)
            .into_iter()
            .filter(|node| contains_offset(node.span, offset))
            .min_by_key(|node| node.span.end.offset.saturating_sub(node.span.start.offset));
        let mut text = format!(
            "{}\ntype: {}",
            definition.name,
            definition_analysis.inferred_type.display()
        );
        if !definition_analysis.capabilities.is_empty() {
            text.push_str("\neffects: ");
            text.push_str(&definition_analysis.capabilities.join(", "));
        }
        if let Some(node) = &node {
            text.push_str("\nnode: ");
            text.push_str(&node.id);
        }
        let range = node.map_or_else(
            || span_range(&self.source, definition.expression.span),
            |node| span_range(&self.source, node.span),
        );
        Some(json!({
            "contents": { "kind": "plaintext", "value": text },
            "range": range,
        }))
    }

    pub(crate) fn definition(&self, line: u64, character: u64) -> Option<Value> {
        let offset = offset_from_lsp(&self.source, line, character)?;
        let program = load_program_source(&self.source).ok()?;
        let symbols = local_symbol_index(&program).ok()?;
        if let Some(target) = symbols.definition_at(offset) {
            return Some(json!({
                "uri": self.uri,
                "range": span_range(&self.source, target),
            }));
        }
        let name = variable_at(&program, offset)?;
        let target = definition_name_spans(&self.source)
            .into_iter()
            .find(|(candidate, _)| candidate == &name)
            .map(|(_, span)| span)?;
        Some(json!({
            "uri": self.uri,
            "range": span_range(&self.source, target),
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

fn contains_offset(span: Span, offset: usize) -> bool {
    span.start.offset <= offset && offset < span.end.offset
}

fn variable_at(program: &Program, offset: usize) -> Option<String> {
    program
        .definitions
        .iter()
        .find_map(|definition| resolve_variable(&definition.expression, offset))
}

fn resolve_variable(expression: &Expression, offset: usize) -> Option<String> {
    if !contains_offset(expression.span, offset) {
        return None;
    }
    let child = match &expression.kind {
        ExpressionKind::If {
            condition,
            consequent,
            alternative,
        } => [
            condition.as_ref(),
            consequent.as_ref(),
            alternative.as_ref(),
        ]
        .into_iter()
        .find_map(|child| resolve_variable(child, offset)),
        ExpressionKind::And(expressions)
        | ExpressionKind::Or(expressions)
        | ExpressionKind::Do(expressions) => expressions
            .iter()
            .find_map(|child| resolve_variable(child, offset)),
        ExpressionKind::Cond {
            clauses,
            alternative,
        } => clauses
            .iter()
            .find_map(|clause| {
                resolve_variable(&clause.condition, offset)
                    .or_else(|| resolve_variable(&clause.expression, offset))
            })
            .or_else(|| resolve_variable(alternative, offset)),
        ExpressionKind::Match { value, arms } => resolve_variable(value, offset).or_else(|| {
            arms.iter()
                .find_map(|arm| resolve_variable(&arm.expression, offset))
        }),
        ExpressionKind::Let {
            bindings: let_bindings,
            body,
        } => {
            let mut found = None;
            for binding in let_bindings {
                found = resolve_variable(&binding.expression, offset);
                if found.is_some() {
                    break;
                }
            }
            found.or_else(|| resolve_variable(body, offset))
        }
        ExpressionKind::Function { body, .. } => resolve_variable(body, offset),
        ExpressionKind::Call { callee, arguments } => {
            resolve_variable(callee, offset).or_else(|| {
                arguments
                    .iter()
                    .find_map(|argument| resolve_variable(argument, offset))
            })
        }
        ExpressionKind::Literal(_) | ExpressionKind::Variable(_) | ExpressionKind::Quote(_) => None,
    };
    child.or_else(|| match &expression.kind {
        ExpressionKind::Variable(name) => Some(name.clone()),
        _ => None,
    })
}

fn definition_name_spans(source: &str) -> Vec<(String, Span)> {
    let Ok(root) = read_source(source, ReaderLimits::default()) else {
        return Vec::new();
    };
    let Some(forms) = root.list() else {
        return Vec::new();
    };
    forms
        .iter()
        .skip(1)
        .filter_map(definition_name_span)
        .collect()
}

fn definition_name_span(form: &Datum) -> Option<(String, Span)> {
    let values = form.list()?;
    let [head, name, ..] = values else {
        return None;
    };
    if head.symbol()? != "def" {
        return None;
    }
    let DatumKind::Symbol(name_value) = &name.kind else {
        return None;
    };
    Some((name_value.clone(), name.span))
}

#[cfg(test)]
mod tests {
    use super::{OpenDocument, offset_from_lsp};

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

    #[test]
    fn utf16_positions_do_not_split_surrogate_pairs() {
        assert_eq!(offset_from_lsp("a😀b", 0, 1), Some(1));
        assert_eq!(offset_from_lsp("a😀b", 0, 2), None);
        assert_eq!(offset_from_lsp("a😀b", 0, 3), Some(5));
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
