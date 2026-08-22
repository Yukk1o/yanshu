use serde_json::json;
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_syntax::{
    DatumKind, Program, ReaderLimits, SymbolBinding, SymbolBindingKind, SymbolIndex,
    load_program_source, read_source, symbol_index,
};

pub(crate) const MAXIMUM_RENAME_EDITS: usize = 1024;
pub(crate) const MAXIMUM_RENAME_TEXT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedRename {
    pub(crate) span: Span,
    pub(crate) placeholder: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenameResult {
    pub(crate) spans: Vec<Span>,
    pub(crate) new_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindingShape {
    name: String,
    kind: SymbolBindingKind,
    declaration: (usize, usize),
    references: Vec<(usize, usize)>,
}

pub(crate) fn prepare_rename_at(program: &Program, offset: usize) -> Option<PreparedRename> {
    let index = symbol_index(program).ok()?;
    let binding = binding_at(&index, offset)?;
    Some(PreparedRename {
        span: occurrence_at(binding, offset)?,
        placeholder: binding.name.clone(),
    })
}

pub(crate) fn rename_at(
    program: &Program,
    offset: usize,
    new_name: &str,
) -> YanshuResult<RenameResult> {
    validate_new_name(new_name)?;
    let index = symbol_index(program).map_err(|diagnostic| {
        unavailable_diagnostic("symbol-index-unavailable", Some(diagnostic.code))
    })?;
    let target_index = index
        .bindings()
        .iter()
        .position(|binding| occurrence_at(binding, offset).is_some())
        .ok_or_else(|| unavailable_diagnostic("not-a-bound-symbol", None))?;
    let target = &index.bindings()[target_index];
    if new_name == target.name {
        return Err(Diagnostic::simple(
            "LSP_RENAME_NAME",
            "rename requires a new symbol name",
        ));
    }

    let mut spans = Vec::with_capacity(target.references.len().saturating_add(1));
    spans.push(target.declaration);
    spans.extend_from_slice(&target.references);
    spans.sort_by_key(span_key);
    validate_occurrences(&program.source, &target.name, &spans, new_name)?;

    let candidate_source = rewrite_source(&program.source, &spans, new_name)?;
    let candidate = load_program_source(&candidate_source)
        .map_err(|diagnostic| conflict_diagnostic("candidate-program-invalid", diagnostic.code))?;
    let candidate_index = symbol_index(&candidate).map_err(|diagnostic| {
        conflict_diagnostic("candidate-symbol-index-invalid", diagnostic.code)
    })?;

    let expected = binding_shapes(&index, Some(target_index), &spans, new_name)?;
    let actual = binding_shapes(&candidate_index, None, &[], "")?;
    if actual != expected {
        return Err(conflict_diagnostic(
            "symbol-resolution-changed",
            "SYMBOL_GRAPH_MISMATCH",
        ));
    }

    Ok(RenameResult {
        spans,
        new_name: new_name.to_owned(),
    })
}

fn binding_at(index: &SymbolIndex, offset: usize) -> Option<&SymbolBinding> {
    index
        .bindings()
        .iter()
        .find(|binding| occurrence_at(binding, offset).is_some())
}

fn occurrence_at(binding: &SymbolBinding, offset: usize) -> Option<Span> {
    std::iter::once(binding.declaration)
        .chain(binding.references.iter().copied())
        .find(|span| span.start.offset <= offset && offset < span.end.offset)
}

fn validate_new_name(new_name: &str) -> YanshuResult<()> {
    let datum = read_source(new_name, ReaderLimits::default()).map_err(|diagnostic| {
        Diagnostic::new(
            "LSP_RENAME_NAME",
            "rename replacement must be exactly one Yanshu symbol",
            json!({ "reason": diagnostic.code }),
        )
    })?;
    if !matches!(&datum.kind, DatumKind::Symbol(name) if name == new_name) {
        return Err(Diagnostic::simple(
            "LSP_RENAME_NAME",
            "rename replacement must be exactly one Yanshu symbol",
        ));
    }
    Ok(())
}

fn validate_occurrences(
    source: &str,
    old_name: &str,
    spans: &[Span],
    new_name: &str,
) -> YanshuResult<()> {
    if spans.len() > MAXIMUM_RENAME_EDITS {
        return Err(Diagnostic::new(
            "LSP_RENAME_LIMIT",
            "rename exceeds the configured edit limit",
            json!({ "actual": spans.len(), "maximum": MAXIMUM_RENAME_EDITS }),
        ));
    }
    let replacement_bytes = new_name.len().checked_mul(spans.len()).ok_or_else(|| {
        Diagnostic::simple(
            "LSP_RENAME_LIMIT",
            "rename replacement byte accounting overflowed",
        )
    })?;
    if replacement_bytes > MAXIMUM_RENAME_TEXT_BYTES {
        return Err(Diagnostic::new(
            "LSP_RENAME_LIMIT",
            "rename exceeds the configured replacement byte limit",
            json!({
                "actual": replacement_bytes,
                "maximum": MAXIMUM_RENAME_TEXT_BYTES,
            }),
        ));
    }

    let mut previous_end = 0;
    for span in spans {
        if span.start.offset < previous_end
            || source.get(span.start.offset..span.end.offset) != Some(old_name)
        {
            return Err(unavailable_diagnostic("source-index-mismatch", None));
        }
        previous_end = span.end.offset;
    }
    Ok(())
}

fn rewrite_source(source: &str, spans: &[Span], new_name: &str) -> YanshuResult<String> {
    let removed_bytes = spans.iter().try_fold(0_usize, |total, span| {
        total.checked_add(span.end.offset.saturating_sub(span.start.offset))
    });
    let replacement_bytes = new_name.len().checked_mul(spans.len());
    let candidate_bytes = removed_bytes
        .and_then(|removed| source.len().checked_sub(removed))
        .and_then(|remaining| replacement_bytes.and_then(|added| remaining.checked_add(added)))
        .ok_or_else(|| {
            Diagnostic::simple(
                "LSP_RENAME_LIMIT",
                "rename candidate byte accounting overflowed",
            )
        })?;
    let maximum = ReaderLimits::default().max_source_bytes;
    if candidate_bytes > maximum {
        return Err(Diagnostic::new(
            "LSP_RENAME_LIMIT",
            "rename candidate exceeds the language source byte limit",
            json!({ "actual": candidate_bytes, "maximum": maximum }),
        ));
    }

    let mut candidate = String::with_capacity(candidate_bytes);
    let mut cursor = 0;
    for span in spans {
        let Some(prefix) = source.get(cursor..span.start.offset) else {
            return Err(unavailable_diagnostic("source-index-mismatch", None));
        };
        candidate.push_str(prefix);
        candidate.push_str(new_name);
        cursor = span.end.offset;
    }
    let Some(suffix) = source.get(cursor..) else {
        return Err(unavailable_diagnostic("source-index-mismatch", None));
    };
    candidate.push_str(suffix);
    if candidate.len() != candidate_bytes {
        return Err(unavailable_diagnostic("candidate-size-mismatch", None));
    }
    Ok(candidate)
}

fn binding_shapes(
    index: &SymbolIndex,
    renamed_binding: Option<usize>,
    edits: &[Span],
    new_name: &str,
) -> YanshuResult<Vec<BindingShape>> {
    index
        .bindings()
        .iter()
        .enumerate()
        .map(|(binding_index, binding)| {
            let mut references = binding
                .references
                .iter()
                .map(|span| transform_span(*span, edits, new_name.len()))
                .collect::<YanshuResult<Vec<_>>>()?;
            references.sort_unstable();
            Ok(BindingShape {
                name: if renamed_binding == Some(binding_index) {
                    new_name.to_owned()
                } else {
                    binding.name.clone()
                },
                kind: binding.kind,
                declaration: transform_span(binding.declaration, edits, new_name.len())?,
                references,
            })
        })
        .collect()
}

fn transform_span(
    span: Span,
    edits: &[Span],
    replacement_length: usize,
) -> YanshuResult<(usize, usize)> {
    Ok((
        transform_offset(span.start.offset, edits, replacement_length)?,
        transform_offset(span.end.offset, edits, replacement_length)?,
    ))
}

fn transform_offset(
    offset: usize,
    edits: &[Span],
    replacement_length: usize,
) -> YanshuResult<usize> {
    let mut transformed = offset;
    for edit in edits {
        if edit.end.offset > offset {
            break;
        }
        transformed = transformed
            .checked_sub(edit.end.offset.saturating_sub(edit.start.offset))
            .and_then(|value| value.checked_add(replacement_length))
            .ok_or_else(|| {
                Diagnostic::simple("LSP_RENAME_LIMIT", "rename span accounting overflowed")
            })?;
    }
    Ok(transformed)
}

fn span_key(span: &Span) -> (usize, usize) {
    (span.start.offset, span.end.offset)
}

fn unavailable_diagnostic(reason: &'static str, source_code: Option<&'static str>) -> Diagnostic {
    Diagnostic::new(
        "LSP_RENAME_UNAVAILABLE",
        "rename is unavailable at this document position",
        json!({ "reason": reason, "sourceCode": source_code }),
    )
}

fn conflict_diagnostic(reason: &'static str, source_code: &'static str) -> Diagnostic {
    Diagnostic::new(
        "LSP_RENAME_CONFLICT",
        "rename would change symbol resolution",
        json!({ "reason": reason, "sourceCode": source_code }),
    )
}

#[cfg(test)]
mod tests {
    use yanshu_syntax::load_program_source;

    use super::{
        MAXIMUM_RENAME_EDITS, MAXIMUM_RENAME_TEXT_BYTES, ReaderLimits, RenameResult,
        prepare_rename_at, rename_at,
    };

    fn program(source: &str) -> yanshu_syntax::Program {
        load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
    }

    fn renamed_source(source: &str, result: &RenameResult) -> String {
        let mut renamed = source.to_owned();
        for span in result.spans.iter().rev() {
            renamed.replace_range(span.start.offset..span.end.offset, result.new_name.as_str());
        }
        renamed
    }

    #[test]
    fn prepares_only_formally_resolved_binding_occurrences() {
        let source =
            "(program (name prepare) (version 1) (def use (fn (value) (list value))) (export use))";
        let program = program(source);
        let value_reference = source
            .rfind("value")
            .unwrap_or_else(|| panic!("value reference missing"));
        let prepared = prepare_rename_at(&program, value_reference)
            .unwrap_or_else(|| panic!("parameter rename was not prepared"));
        assert_eq!(prepared.placeholder, "value");
        assert_eq!(
            &source[prepared.span.start.offset..prepared.span.end.offset],
            "value"
        );

        let primitive = source
            .find("list")
            .unwrap_or_else(|| panic!("primitive fixture missing"));
        assert_eq!(prepare_rename_at(&program, primitive), None);
        let program_name = source
            .find("prepare")
            .unwrap_or_else(|| panic!("program name missing"));
        assert_eq!(prepare_rename_at(&program, program_name), None);
    }

    #[test]
    fn renames_one_global_binding_across_structural_reference_sites() {
        let source = "(program (name global-rename) (version 4) (signature target (fn (integer) integer)) (def target (fn (value) value)) (signature use (fn (integer) integer)) (def use (fn (value) (target value))) (export target use))";
        let program = program(source);
        let call = source
            .rfind("(target value)")
            .unwrap_or_else(|| panic!("target call missing"))
            + 1;
        let result = rename_at(&program, call, "renamed")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(result.spans.len(), 4);
        let renamed = renamed_source(source, &result);
        assert_eq!(renamed, source.replace("target", "renamed"));
        let _program =
            load_program_source(&renamed).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    }

    #[test]
    fn renames_a_local_binding_without_touching_outer_names() {
        let source = "(program (name local-rename) (version 1) (def use (fn (outer) (let ((value outer)) (list value outer)))) (export use))";
        let program = program(source);
        let reference = source
            .find("(list value outer)")
            .unwrap_or_else(|| panic!("local reference missing"))
            + "(list ".len();
        let result = rename_at(&program, reference, "item")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(result.spans.len(), 2);
        assert_eq!(
            renamed_source(source, &result),
            "(program (name local-rename) (version 1) (def use (fn (outer) (let ((item outer)) (list item outer)))) (export use))"
        );
    }

    #[test]
    fn renames_parameters_and_pattern_bindings_in_their_own_scopes() {
        let parameter_source = "(program (name parameter-rename) (version 1) (def use (fn (value) (list value value))) (export use))";
        let parameter_program = program(parameter_source);
        let parameter_reference = parameter_source
            .rfind("value")
            .unwrap_or_else(|| panic!("parameter reference missing"));
        let parameter_result = rename_at(&parameter_program, parameter_reference, "item")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(parameter_result.spans.len(), 3);
        assert_eq!(
            renamed_source(parameter_source, &parameter_result),
            "(program (name parameter-rename) (version 1) (def use (fn (item) (list item item))) (export use))"
        );

        let pattern_source = "(program (name pattern-rename) (version 3) (data decision (approved amount)) (def inspect (fn (decision) (match decision ((approved amount) amount) (_ decision)))) (export inspect approved))";
        let pattern_program = program(pattern_source);
        let pattern_reference = pattern_source
            .find("(approved amount) amount")
            .unwrap_or_else(|| panic!("pattern reference fixture missing"))
            + "(approved amount) ".len();
        let pattern_result = rename_at(&pattern_program, pattern_reference, "value")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(pattern_result.spans.len(), 2);
        assert_eq!(
            renamed_source(pattern_source, &pattern_result),
            "(program (name pattern-rename) (version 3) (data decision (approved amount)) (def inspect (fn (decision) (match decision ((approved value) value) (_ decision)))) (export inspect approved))"
        );
    }

    #[test]
    fn leaves_equal_text_in_strings_quotes_and_comments_unchanged() {
        let source = "(program (name data-is-not-code) (version 1) ; target comment\n  (def target (fn (value) (list \"target\" 'target value))) (def use (fn (value) (target value))) (export target use))";
        let program = program(source);
        let call = source
            .rfind("(target value)")
            .unwrap_or_else(|| panic!("target call missing"))
            + 1;
        let result = rename_at(&program, call, "renamed")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let renamed = renamed_source(source, &result);
        assert!(renamed.contains("; target comment"));
        assert!(renamed.contains("\"target\""));
        assert!(renamed.contains("'target"));
        assert!(renamed.contains("(def renamed"));
        assert!(renamed.contains("(renamed value)"));
        assert!(renamed.contains("(export renamed use)"));
    }

    #[test]
    fn rejects_capture_in_both_lexical_directions() {
        let source = "(program (name capture) (version 1) (def use (fn (outer) (let ((value outer)) (list value outer)))) (export use))";
        let program = program(source);
        let outer_reference = source
            .rfind("outer")
            .unwrap_or_else(|| panic!("outer body reference missing"));
        let outer_error = rename_at(&program, outer_reference, "value")
            .err()
            .unwrap_or_else(|| panic!("outer rename unexpectedly allowed capture"));
        assert_eq!(outer_error.code, "LSP_RENAME_CONFLICT");

        let value_reference = source
            .find("(list value outer)")
            .unwrap_or_else(|| panic!("value body reference missing"))
            + "(list ".len();
        let inner_error = rename_at(&program, value_reference, "outer")
            .err()
            .unwrap_or_else(|| panic!("inner rename unexpectedly allowed capture"));
        assert_eq!(inner_error.code, "LSP_RENAME_CONFLICT");
    }

    #[test]
    fn allows_the_same_name_in_disjoint_lexical_scopes() {
        let source = "(program (name disjoint) (version 1) (def use (fn () (if #t (fn (left) left) (fn (right) right)))) (export use))";
        let program = program(source);
        let left_reference = source
            .find("left)")
            .unwrap_or_else(|| panic!("left reference missing"));
        let result = rename_at(&program, left_reference, "right")
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            renamed_source(source, &result),
            "(program (name disjoint) (version 1) (def use (fn () (if #t (fn (right) right) (fn (right) right)))) (export use))"
        );
    }

    #[test]
    fn rejects_non_symbol_and_unchanged_replacement_names() {
        let source = "(program (name names) (version 1) (def use (fn (value) value)) (export use))";
        let program = program(source);
        let reference = source
            .rfind("value")
            .unwrap_or_else(|| panic!("value reference missing"));
        for invalid in [
            "",
            "two names",
            "(name)",
            "\"name\"",
            "#t",
            "42",
            "; comment",
        ] {
            let diagnostic = rename_at(&program, reference, invalid)
                .err()
                .unwrap_or_else(|| panic!("invalid rename name unexpectedly accepted"));
            assert_eq!(diagnostic.code, "LSP_RENAME_NAME");
        }
        let unchanged = rename_at(&program, reference, "value")
            .err()
            .unwrap_or_else(|| panic!("unchanged rename unexpectedly accepted"));
        assert_eq!(unchanged.code, "LSP_RENAME_NAME");
    }

    #[test]
    fn fails_before_building_an_unbounded_edit_response() {
        let mut source =
            String::from("(program (name bounded-rename) (version 1) (def use (fn (value) (list");
        for _ in 0..MAXIMUM_RENAME_EDITS {
            source.push_str(" value");
        }
        source.push_str("))) (export use))");
        let program = program(&source);
        let reference = source
            .rfind("value")
            .unwrap_or_else(|| panic!("bounded reference missing"));
        let diagnostic = rename_at(&program, reference, "item")
            .err()
            .unwrap_or_else(|| panic!("oversized rename unexpectedly succeeded"));
        assert_eq!(diagnostic.code, "LSP_RENAME_LIMIT");
        assert_eq!(
            diagnostic.details.as_ref(),
            &serde_json::json!({
                "actual": MAXIMUM_RENAME_EDITS + 1,
                "maximum": MAXIMUM_RENAME_EDITS,
            })
        );
    }

    #[test]
    fn fails_before_building_unbounded_replacement_text() {
        let mut source =
            String::from("(program (name bounded-text) (version 1) (def use (fn (value) (list");
        let references = 64;
        for _ in 0..references {
            source.push_str(" value");
        }
        source.push_str("))) (export use))");
        let program = program(&source);
        let reference = source
            .rfind("value")
            .unwrap_or_else(|| panic!("bounded text reference missing"));
        let new_name = "x".repeat(ReaderLimits::default().max_token_bytes);
        let diagnostic = rename_at(&program, reference, &new_name)
            .err()
            .unwrap_or_else(|| panic!("oversized replacement text unexpectedly succeeded"));
        assert_eq!(diagnostic.code, "LSP_RENAME_LIMIT");
        assert_eq!(
            diagnostic.details.as_ref(),
            &serde_json::json!({
                "actual": new_name.len() * (references + 1),
                "maximum": MAXIMUM_RENAME_TEXT_BYTES,
            })
        );
    }
}
