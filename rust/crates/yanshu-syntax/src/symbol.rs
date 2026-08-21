use std::collections::BTreeMap;

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};

use crate::{
    Datum, Expression, ExpressionKind, Pattern, PatternKind, Program, ReaderLimits, read_source,
};

type SpanKey = (usize, usize);

/// The lexical construct that introduced a local guest binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalBindingKind {
    Parameter,
    Let,
    Pattern,
}

/// One local declaration and all variable expressions resolved to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBinding {
    pub name: String,
    pub kind: LocalBindingKind,
    pub declaration: Span,
    pub references: Vec<Span>,
}

/// A bounded, deterministic lexical binding index for one parsed program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSymbolIndex {
    bindings: Vec<LocalBinding>,
}

impl LocalSymbolIndex {
    #[must_use]
    pub fn bindings(&self) -> &[LocalBinding] {
        &self.bindings
    }

    /// Resolves either a local declaration or one of its references.
    #[must_use]
    pub fn definition_at(&self, offset: usize) -> Option<Span> {
        self.bindings.iter().find_map(|binding| {
            (contains_offset(binding.declaration, offset)
                || binding
                    .references
                    .iter()
                    .any(|reference| contains_offset(*reference, offset)))
            .then_some(binding.declaration)
        })
    }
}

/// Builds the local lexical symbol graph from the canonical parsed program.
///
/// The AST defines scope and resolution. A second bounded Reader pass over the
/// same source supplies declaration spans that are intentionally absent from
/// executable semantics. A mismatch fails closed instead of guessing offsets.
pub fn local_symbol_index(program: &Program) -> YanshuResult<LocalSymbolIndex> {
    let root = read_source(&program.source, ReaderLimits::default())?;
    let mut declarations = DeclarationSpans::default();
    collect_declaration_spans(&root, &mut declarations);

    let mut builder = SymbolBuilder {
        source: &program.source,
        declarations: &declarations,
        bindings: Vec::new(),
        scope_stack: Vec::new(),
        active_bindings: BTreeMap::new(),
        occurrence_count: 0,
        occurrence_limit: ReaderLimits::default().max_nodes,
    };
    for definition in &program.definitions {
        builder.visit_expression(&definition.expression)?;
    }
    Ok(LocalSymbolIndex {
        bindings: builder.bindings,
    })
}

#[derive(Default)]
struct DeclarationSpans {
    function_parameters: BTreeMap<SpanKey, Vec<Span>>,
    let_bindings: BTreeMap<SpanKey, Vec<Span>>,
}

fn collect_declaration_spans(datum: &Datum, declarations: &mut DeclarationSpans) {
    let Some(form) = datum.list() else {
        return;
    };
    match form.first().and_then(Datum::symbol) {
        Some("fn") if form.len() == 3 => {
            if let Some(parameters) = form[1].list() {
                declarations.function_parameters.insert(
                    span_key(datum.span),
                    parameters.iter().map(|parameter| parameter.span).collect(),
                );
            }
        }
        Some("let") if form.len() == 3 => {
            if let Some(bindings) = form[1].list() {
                declarations.let_bindings.insert(
                    span_key(datum.span),
                    bindings
                        .iter()
                        .filter_map(|binding| binding.list()?.first().map(|name| name.span))
                        .collect(),
                );
            }
        }
        _ => {}
    }
    for child in form {
        collect_declaration_spans(child, declarations);
    }
}

struct SymbolBuilder<'declarations> {
    source: &'declarations str,
    declarations: &'declarations DeclarationSpans,
    bindings: Vec<LocalBinding>,
    scope_stack: Vec<String>,
    active_bindings: BTreeMap<String, Vec<usize>>,
    occurrence_count: usize,
    occurrence_limit: usize,
}

impl SymbolBuilder<'_> {
    fn visit_expression(&mut self, expression: &Expression) -> YanshuResult<()> {
        match &expression.kind {
            ExpressionKind::Literal(_) | ExpressionKind::Quote(_) => Ok(()),
            ExpressionKind::Variable(name) => self.record_reference(name, expression.span),
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => {
                self.visit_expression(condition)?;
                self.visit_expression(consequent)?;
                self.visit_expression(alternative)
            }
            ExpressionKind::And(expressions)
            | ExpressionKind::Or(expressions)
            | ExpressionKind::Do(expressions) => self.visit_expressions(expressions),
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => {
                for clause in clauses {
                    self.visit_expression(&clause.condition)?;
                    self.visit_expression(&clause.expression)?;
                }
                self.visit_expression(alternative)
            }
            ExpressionKind::Match { value, arms } => {
                self.visit_expression(value)?;
                for arm in arms {
                    let outer_scope = self.scope_stack.len();
                    self.bind_pattern(&arm.pattern)?;
                    self.visit_expression(&arm.expression)?;
                    self.restore_scope(outer_scope);
                }
                Ok(())
            }
            ExpressionKind::Let { bindings, body } => {
                let spans = self
                    .declarations
                    .let_bindings
                    .get(&span_key(expression.span))
                    .filter(|spans| spans.len() == bindings.len())
                    .ok_or_else(|| source_mismatch("let binding", expression.span))?;
                let outer_scope = self.scope_stack.len();
                for (binding, declaration) in bindings.iter().zip(spans) {
                    self.visit_expression(&binding.expression)?;
                    self.bind(&binding.name, LocalBindingKind::Let, *declaration)?;
                }
                self.visit_expression(body)?;
                self.restore_scope(outer_scope);
                Ok(())
            }
            ExpressionKind::Function { parameters, body } => {
                let spans = self
                    .declarations
                    .function_parameters
                    .get(&span_key(expression.span))
                    .filter(|spans| spans.len() == parameters.len())
                    .ok_or_else(|| source_mismatch("function parameter", expression.span))?;
                let outer_scope = self.scope_stack.len();
                for (parameter, declaration) in parameters.iter().zip(spans) {
                    self.bind(parameter, LocalBindingKind::Parameter, *declaration)?;
                }
                self.visit_expression(body)?;
                self.restore_scope(outer_scope);
                Ok(())
            }
            ExpressionKind::Call { callee, arguments } => {
                self.visit_expression(callee)?;
                self.visit_expressions(arguments)
            }
        }
    }

    fn visit_expressions(&mut self, expressions: &[Expression]) -> YanshuResult<()> {
        for expression in expressions {
            self.visit_expression(expression)?;
        }
        Ok(())
    }

    fn bind_pattern(&mut self, pattern: &Pattern) -> YanshuResult<()> {
        match &pattern.kind {
            PatternKind::Binding(name) => self.bind(name, LocalBindingKind::Pattern, pattern.span),
            PatternKind::Variant { fields, .. } => {
                for field in fields {
                    self.bind_pattern(field)?;
                }
                Ok(())
            }
            PatternKind::Wildcard | PatternKind::Literal(_) => Ok(()),
        }
    }

    fn bind(&mut self, name: &str, kind: LocalBindingKind, declaration: Span) -> YanshuResult<()> {
        if self
            .source
            .get(declaration.start.offset..declaration.end.offset)
            != Some(name)
        {
            return Err(source_mismatch("local binding name", declaration));
        }
        self.charge_occurrence()?;
        let identifier = self.bindings.len();
        self.bindings.push(LocalBinding {
            name: name.to_owned(),
            kind,
            declaration,
            references: Vec::new(),
        });
        self.active_bindings
            .entry(name.to_owned())
            .or_default()
            .push(identifier);
        self.scope_stack.push(name.to_owned());
        Ok(())
    }

    fn record_reference(&mut self, name: &str, reference: Span) -> YanshuResult<()> {
        let Some(identifier) = self
            .active_bindings
            .get(name)
            .and_then(|identifiers| identifiers.last())
            .copied()
        else {
            return Ok(());
        };
        self.charge_occurrence()?;
        self.bindings[identifier].references.push(reference);
        Ok(())
    }

    fn restore_scope(&mut self, outer_scope: usize) {
        while self.scope_stack.len() > outer_scope {
            let Some(name) = self.scope_stack.pop() else {
                break;
            };
            let remove = self
                .active_bindings
                .get_mut(&name)
                .is_some_and(|identifiers| {
                    let _identifier = identifiers.pop();
                    identifiers.is_empty()
                });
            if remove {
                self.active_bindings.remove(&name);
            }
        }
    }

    fn charge_occurrence(&mut self) -> YanshuResult<()> {
        self.occurrence_count = self.occurrence_count.checked_add(1).ok_or_else(|| {
            Diagnostic::simple(
                "TOOL_SYMBOL_LIMIT",
                "local symbol occurrence accounting overflowed",
            )
        })?;
        if self.occurrence_count > self.occurrence_limit {
            return Err(Diagnostic::new(
                "TOOL_SYMBOL_LIMIT",
                "local symbol occurrences exceed the Reader node limit",
                json!({
                    "actual": self.occurrence_count,
                    "maximum": self.occurrence_limit,
                }),
            ));
        }
        Ok(())
    }
}

fn source_mismatch(kind: &'static str, span: Span) -> Diagnostic {
    Diagnostic::new(
        "TOOL_SYMBOL_SOURCE_MISMATCH",
        "parsed syntax and local declaration spans do not agree",
        json!({ "kind": kind }),
    )
    .at(span)
}

fn span_key(span: Span) -> SpanKey {
    (span.start.offset, span.end.offset)
}

fn contains_offset(span: Span, offset: usize) -> bool {
    span.start.offset <= offset && offset < span.end.offset
}

#[cfg(test)]
mod tests {
    use super::{LocalBindingKind, local_symbol_index};
    use crate::load_program_source;

    fn parsed(source: &str) -> crate::Program {
        load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
    }

    #[test]
    fn resolves_parameters_and_sequential_let_shadowing() {
        let source = "(program (name symbols) (version 1) (def use (fn (x) (let ((before x) (x before)) (+ x before)))) (export use))";
        let index =
            local_symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(index.bindings().len(), 3);

        let parameter = &index.bindings()[0];
        assert_eq!(parameter.name, "x");
        assert_eq!(parameter.kind, LocalBindingKind::Parameter);
        assert_eq!(parameter.references.len(), 1);

        let before = &index.bindings()[1];
        assert_eq!(before.name, "before");
        assert_eq!(before.kind, LocalBindingKind::Let);
        assert_eq!(before.references.len(), 2);

        let shadow = &index.bindings()[2];
        assert_eq!(shadow.name, "x");
        assert_eq!(shadow.kind, LocalBindingKind::Let);
        assert_eq!(shadow.references.len(), 1);

        for binding in index.bindings() {
            assert_eq!(
                &source[binding.declaration.start.offset..binding.declaration.end.offset],
                binding.name,
            );
            for reference in &binding.references {
                assert_eq!(
                    &source[reference.start.offset..reference.end.offset],
                    binding.name,
                );
                assert_eq!(
                    index.definition_at(reference.start.offset),
                    Some(binding.declaration)
                );
            }
        }
    }

    #[test]
    fn scopes_pattern_bindings_to_one_match_arm() {
        let source = "(program (name patterns) (version 3) (data decision (approved amount)) (def inspect (fn (decision) (match decision ((approved amount) amount) (_ decision)))) (export inspect approved))";
        let index =
            local_symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let amount = index
            .bindings()
            .iter()
            .find(|binding| binding.kind == LocalBindingKind::Pattern)
            .unwrap_or_else(|| panic!("pattern binding missing"));
        assert_eq!(amount.name, "amount");
        assert_eq!(amount.references.len(), 1);
        assert_eq!(
            index.definition_at(amount.references[0].start.offset),
            Some(amount.declaration),
        );

        let parameter = index
            .bindings()
            .iter()
            .find(|binding| binding.kind == LocalBindingKind::Parameter)
            .unwrap_or_else(|| panic!("parameter binding missing"));
        assert_eq!(parameter.name, "decision");
        assert_eq!(parameter.references.len(), 2);
    }

    #[test]
    fn restores_outer_scope_after_a_nested_function() {
        let source = "(program (name nested) (version 1) (def use (fn (value) (list value ((fn (value) value) 1) value))) (export use))";
        let index =
            local_symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(index.bindings().len(), 2);
        let outer = &index.bindings()[0];
        let inner = &index.bindings()[1];
        assert_eq!(outer.name, "value");
        assert_eq!(outer.references.len(), 2);
        assert_eq!(inner.name, "value");
        assert_eq!(inner.references.len(), 1);
        assert_eq!(
            index.definition_at(inner.references[0].start.offset),
            Some(inner.declaration),
        );
        assert_eq!(
            index.definition_at(outer.references[1].start.offset),
            Some(outer.declaration),
        );
    }

    #[test]
    fn rejects_an_ast_name_that_no_longer_matches_canonical_source() {
        let source =
            "(program (name mismatch) (version 1) (def use (fn (value) value)) (export use))";
        let mut program = parsed(source);
        let crate::ExpressionKind::Function { parameters, .. } =
            &mut program.definitions[0].expression.kind
        else {
            panic!("function fixture missing");
        };
        parameters[0] = "changed".to_owned();
        let diagnostic = local_symbol_index(&program)
            .err()
            .unwrap_or_else(|| panic!("mismatched AST unexpectedly indexed"));
        assert_eq!(diagnostic.code, "TOOL_SYMBOL_SOURCE_MISMATCH");
    }
}
