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

/// The lexical construct that introduced a guest binding visible to tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolBindingKind {
    Definition,
    Parameter,
    Let,
    Pattern,
}

/// One global or local declaration and every variable expression resolved to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolBinding {
    pub name: String,
    pub kind: SymbolBindingKind,
    pub declaration: Span,
    pub references: Vec<Span>,
}

/// A bounded, deterministic symbol index for one parsed program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolIndex {
    bindings: Vec<SymbolBinding>,
}

impl SymbolIndex {
    #[must_use]
    pub fn bindings(&self) -> &[SymbolBinding] {
        &self.bindings
    }

    /// Resolves either a declaration or one of its references.
    #[must_use]
    pub fn definition_at(&self, offset: usize) -> Option<Span> {
        self.binding_at(offset).map(|binding| binding.declaration)
    }

    /// Returns all references for the binding at `offset`, in source order.
    #[must_use]
    pub fn references_at(&self, offset: usize, include_declaration: bool) -> Option<Vec<Span>> {
        let binding = self.binding_at(offset)?;
        let mut spans = Vec::with_capacity(
            binding
                .references
                .len()
                .saturating_add(usize::from(include_declaration)),
        );
        if include_declaration {
            spans.push(binding.declaration);
        }
        spans.extend_from_slice(&binding.references);
        spans.sort_by_key(|span| (span.start.offset, span.end.offset));
        Some(spans)
    }

    fn binding_at(&self, offset: usize) -> Option<&SymbolBinding> {
        self.bindings.iter().find(|binding| {
            contains_offset(binding.declaration, offset)
                || binding
                    .references
                    .iter()
                    .any(|reference| contains_offset(*reference, offset))
        })
    }
}

/// Builds the local lexical symbol graph from the canonical parsed program.
///
/// The AST defines scope and resolution. A second bounded Reader pass over the
/// same source supplies declaration spans that are intentionally absent from
/// executable semantics. A mismatch fails closed instead of guessing offsets.
pub fn local_symbol_index(program: &Program) -> YanshuResult<LocalSymbolIndex> {
    let bindings = build_symbol_bindings(program, false)?
        .into_iter()
        .filter_map(|binding| {
            let kind = match binding.kind {
                SymbolBindingKind::Definition => return None,
                SymbolBindingKind::Parameter => LocalBindingKind::Parameter,
                SymbolBindingKind::Let => LocalBindingKind::Let,
                SymbolBindingKind::Pattern => LocalBindingKind::Pattern,
            };
            Some(LocalBinding {
                name: binding.name,
                kind,
                declaration: binding.declaration,
                references: binding.references,
            })
        })
        .collect();
    Ok(LocalSymbolIndex { bindings })
}

/// Builds a same-program global and local symbol graph from canonical parsed source.
///
/// Global references are variable expressions whose names resolve to a `def`
/// after applying the same local lexical shadowing rules as execution. A
/// definition's signature, route handler, and export sites are also semantic
/// references. Type, schema, quoted, string, and comment names are not.
pub fn symbol_index(program: &Program) -> YanshuResult<SymbolIndex> {
    Ok(SymbolIndex {
        bindings: build_symbol_bindings(program, true)?,
    })
}

fn build_symbol_bindings(
    program: &Program,
    include_definitions: bool,
) -> YanshuResult<Vec<SymbolBinding>> {
    let root = read_source(&program.source, ReaderLimits::default())?;
    let mut declarations = DeclarationSpans::default();
    collect_program_spans(&root, &mut declarations);
    collect_declaration_spans(&root, &mut declarations);

    let mut builder = SymbolBuilder {
        source: &program.source,
        declarations: &declarations,
        bindings: Vec::new(),
        scope_stack: Vec::new(),
        active_bindings: BTreeMap::new(),
        global_bindings: BTreeMap::new(),
        occurrence_count: 0,
        occurrence_limit: ReaderLimits::default().max_nodes,
        include_definitions,
    };
    if include_definitions {
        builder.bind_definitions(program)?;
        builder.record_program_references(program)?;
    }
    for definition in &program.definitions {
        builder.visit_expression(&definition.expression)?;
    }
    Ok(builder.bindings)
}

#[derive(Default)]
struct DeclarationSpans {
    definition_names: Vec<Span>,
    signature_names: Vec<Span>,
    route_handlers: Vec<Span>,
    export_names: Vec<Span>,
    function_parameters: BTreeMap<SpanKey, Vec<Span>>,
    let_bindings: BTreeMap<SpanKey, Vec<Span>>,
}

fn collect_program_spans(datum: &Datum, declarations: &mut DeclarationSpans) {
    let Some(program) = datum.list() else {
        return;
    };
    for member in program.iter().skip(1) {
        let Some(form) = member.list() else {
            continue;
        };
        match form.first().and_then(Datum::symbol) {
            Some("def") => {
                if let Some(name) = form.get(1) {
                    declarations.definition_names.push(name.span);
                }
            }
            Some("signature") => {
                if let Some(name) = form.get(1) {
                    declarations.signature_names.push(name.span);
                }
            }
            Some("route") => {
                if let Some(handler) = form.get(3) {
                    declarations.route_handlers.push(handler.span);
                }
            }
            Some("export") => declarations
                .export_names
                .extend(form.iter().skip(1).map(|name| name.span)),
            _ => {}
        }
    }
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
    bindings: Vec<SymbolBinding>,
    scope_stack: Vec<String>,
    active_bindings: BTreeMap<String, Vec<usize>>,
    global_bindings: BTreeMap<String, usize>,
    occurrence_count: usize,
    occurrence_limit: usize,
    include_definitions: bool,
}

impl SymbolBuilder<'_> {
    fn bind_definitions(&mut self, program: &Program) -> YanshuResult<()> {
        if self.declarations.definition_names.len() != program.definitions.len() {
            let diagnostic = definition_source_mismatch_without_span("definition declaration");
            return Err(program
                .definitions
                .first()
                .map_or(diagnostic.clone(), |definition| {
                    diagnostic.at(definition.expression.span)
                }));
        }
        for (definition, declaration) in program
            .definitions
            .iter()
            .zip(&self.declarations.definition_names)
        {
            self.bind_global(&definition.name, *declaration)?;
        }
        Ok(())
    }

    fn record_program_references(&mut self, program: &Program) -> YanshuResult<()> {
        self.validate_program_span_count(
            "signature reference",
            program.signatures.len(),
            self.declarations.signature_names.len(),
        )?;
        for (index, signature) in program.signatures.iter().enumerate() {
            self.record_global_reference(
                &signature.name,
                self.declarations.signature_names[index],
                "signature reference",
            )?;
        }

        self.validate_program_span_count(
            "route handler reference",
            program.routes.len(),
            self.declarations.route_handlers.len(),
        )?;
        for (index, route) in program.routes.iter().enumerate() {
            self.record_global_reference(
                &route.handler,
                self.declarations.route_handlers[index],
                "route handler reference",
            )?;
        }

        self.validate_program_span_count(
            "export reference",
            program.exports.len(),
            self.declarations.export_names.len(),
        )?;
        for (index, name) in program.exports.iter().enumerate() {
            self.record_global_reference(
                name,
                self.declarations.export_names[index],
                "export reference",
            )?;
        }
        Ok(())
    }

    fn validate_program_span_count(
        &self,
        kind: &'static str,
        expected: usize,
        actual: usize,
    ) -> YanshuResult<()> {
        if expected != actual {
            return Err(definition_source_mismatch_without_span(kind));
        }
        Ok(())
    }

    fn record_global_reference(
        &mut self,
        name: &str,
        reference: Span,
        kind: &'static str,
    ) -> YanshuResult<()> {
        if self
            .source
            .get(reference.start.offset..reference.end.offset)
            != Some(name)
        {
            return Err(definition_source_mismatch(kind, reference));
        }
        let Some(identifier) = self.global_bindings.get(name).copied() else {
            return Ok(());
        };
        self.charge_occurrence()?;
        self.bindings[identifier].references.push(reference);
        Ok(())
    }

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
                    self.bind_local(&binding.name, SymbolBindingKind::Let, *declaration)?;
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
                    self.bind_local(parameter, SymbolBindingKind::Parameter, *declaration)?;
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
            PatternKind::Binding(name) => {
                self.bind_local(name, SymbolBindingKind::Pattern, pattern.span)
            }
            PatternKind::Variant { fields, .. } => {
                for field in fields {
                    self.bind_pattern(field)?;
                }
                Ok(())
            }
            PatternKind::Wildcard | PatternKind::Literal(_) => Ok(()),
        }
    }

    fn bind_global(&mut self, name: &str, declaration: Span) -> YanshuResult<()> {
        if self
            .source
            .get(declaration.start.offset..declaration.end.offset)
            != Some(name)
        {
            return Err(definition_source_mismatch("definition name", declaration));
        }
        self.charge_occurrence()?;
        let identifier = self.bindings.len();
        self.bindings.push(SymbolBinding {
            name: name.to_owned(),
            kind: SymbolBindingKind::Definition,
            declaration,
            references: Vec::new(),
        });
        if self
            .global_bindings
            .insert(name.to_owned(), identifier)
            .is_some()
        {
            return Err(definition_source_mismatch(
                "duplicate definition name",
                declaration,
            ));
        }
        Ok(())
    }

    fn bind_local(
        &mut self,
        name: &str,
        kind: SymbolBindingKind,
        declaration: Span,
    ) -> YanshuResult<()> {
        self.validate_name(name, declaration)?;
        debug_assert!(kind != SymbolBindingKind::Definition);
        self.charge_occurrence()?;
        let identifier = self.bindings.len();
        self.bindings.push(SymbolBinding {
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

    fn validate_name(&self, name: &str, declaration: Span) -> YanshuResult<()> {
        if self
            .source
            .get(declaration.start.offset..declaration.end.offset)
            != Some(name)
        {
            return Err(source_mismatch("local binding name", declaration));
        }
        Ok(())
    }

    fn record_reference(&mut self, name: &str, reference: Span) -> YanshuResult<()> {
        let identifier = self
            .active_bindings
            .get(name)
            .and_then(|identifiers| identifiers.last())
            .copied()
            .or_else(|| self.global_bindings.get(name).copied());
        let Some(identifier) = identifier else {
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
            let message = if self.include_definitions {
                "symbol occurrence accounting overflowed"
            } else {
                "local symbol occurrence accounting overflowed"
            };
            Diagnostic::simple("TOOL_SYMBOL_LIMIT", message)
        })?;
        if self.occurrence_count > self.occurrence_limit {
            let message = if self.include_definitions {
                "symbol occurrences exceed the Reader node limit"
            } else {
                "local symbol occurrences exceed the Reader node limit"
            };
            return Err(Diagnostic::new(
                "TOOL_SYMBOL_LIMIT",
                message,
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
    source_mismatch_without_span(kind).at(span)
}

fn source_mismatch_without_span(kind: &'static str) -> Diagnostic {
    Diagnostic::new(
        "TOOL_SYMBOL_SOURCE_MISMATCH",
        "parsed syntax and local declaration spans do not agree",
        json!({ "kind": kind }),
    )
}

fn definition_source_mismatch(kind: &'static str, span: Span) -> Diagnostic {
    definition_source_mismatch_without_span(kind).at(span)
}

fn definition_source_mismatch_without_span(kind: &'static str) -> Diagnostic {
    Diagnostic::new(
        "TOOL_SYMBOL_SOURCE_MISMATCH",
        "parsed syntax and definition declaration spans do not agree",
        json!({ "kind": kind }),
    )
}

fn span_key(span: Span) -> SpanKey {
    (span.start.offset, span.end.offset)
}

fn contains_offset(span: Span, offset: usize) -> bool {
    span.start.offset <= offset && offset < span.end.offset
}

#[cfg(test)]
mod tests {
    use super::{LocalBindingKind, SymbolBindingKind, local_symbol_index, symbol_index};
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

    #[test]
    fn resolves_global_references_without_crossing_local_shadowing() {
        let source = "(program (name globals) (version 1) (def target (fn (value) value)) (def shadow (fn (target) (list target ((fn (target) target) target)))) (def call (fn (value) (target value))) (export target shadow call))";
        let index =
            symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let target = index
            .bindings()
            .iter()
            .find(|binding| {
                binding.kind == SymbolBindingKind::Definition && binding.name == "target"
            })
            .unwrap_or_else(|| panic!("global target binding missing"));
        assert_eq!(target.references.len(), 2);
        assert!(target.references.iter().any(|reference| {
            reference.start.offset
                > source
                    .find("(def call")
                    .unwrap_or_else(|| panic!("call definition missing"))
        }));

        let shadowing_parameters = index
            .bindings()
            .iter()
            .filter(|binding| {
                binding.kind == SymbolBindingKind::Parameter && binding.name == "target"
            })
            .collect::<Vec<_>>();
        assert_eq!(shadowing_parameters.len(), 2);
        assert_eq!(shadowing_parameters[0].references.len(), 2);
        assert_eq!(shadowing_parameters[1].references.len(), 1);
    }

    #[test]
    fn returns_sorted_references_and_honors_include_declaration() {
        let source = "(program (name references) (version 1) (def use (fn (value) (list value value))) (export use))";
        let index =
            symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let value = index
            .bindings()
            .iter()
            .find(|binding| binding.kind == SymbolBindingKind::Parameter && binding.name == "value")
            .unwrap_or_else(|| panic!("parameter binding missing"));
        let without_declaration = index
            .references_at(value.references[0].start.offset, false)
            .unwrap_or_else(|| panic!("references missing"));
        assert_eq!(without_declaration, value.references);

        let with_declaration = index
            .references_at(value.declaration.start.offset, true)
            .unwrap_or_else(|| panic!("declaration references missing"));
        assert_eq!(with_declaration.len(), 3);
        assert_eq!(with_declaration[0], value.declaration);
        assert!(
            with_declaration
                .windows(2)
                .all(|pair| pair[0].start.offset < pair[1].start.offset)
        );
    }

    #[test]
    fn rejects_a_global_definition_name_that_disagrees_with_source() {
        let source =
            "(program (name mismatch) (version 1) (def use (fn (value) value)) (export use))";
        let mut program = parsed(source);
        program.definitions[0].name = "changed".to_owned();
        let diagnostic = symbol_index(&program)
            .err()
            .unwrap_or_else(|| panic!("mismatched global definition unexpectedly indexed"));
        assert_eq!(diagnostic.code, "TOOL_SYMBOL_SOURCE_MISMATCH");
    }

    #[test]
    fn ignores_textual_names_that_are_not_variable_references() {
        let source = r#"(program
  ; target in a comment is not a reference
  (name textual)
  (version 1)
  (def target (fn (value) value))
  (def use (fn (value) (list "target" (quote target) (target value))))
  (export use))"#;
        let index =
            symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let target = index
            .bindings()
            .iter()
            .find(|binding| {
                binding.kind == SymbolBindingKind::Definition && binding.name == "target"
            })
            .unwrap_or_else(|| panic!("target binding missing"));
        assert_eq!(target.references.len(), 1);
        assert_eq!(
            &source[target.references[0].start.offset..target.references[0].end.offset],
            "target"
        );
        assert!(
            target.references[0].start.offset
                > source
                    .find("(quote target)")
                    .unwrap_or_else(|| panic!("quoted target missing"))
        );
    }

    #[test]
    fn includes_signature_route_and_export_definition_references() {
        let source = r#"(program
  (name entrypoints)
  (version 4)
  (signature target (fn (integer) integer))
  (def target (fn (value) value))
  (signature use (fn (integer) integer))
  (def use (fn (value) (target value)))
  (route GET "/target" target)
  (export target use))"#;
        let index =
            symbol_index(&parsed(source)).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let target = index
            .bindings()
            .iter()
            .find(|binding| {
                binding.kind == SymbolBindingKind::Definition && binding.name == "target"
            })
            .unwrap_or_else(|| panic!("target definition missing"));
        assert_eq!(target.references.len(), 4);
        for reference in &target.references {
            assert_eq!(
                &source[reference.start.offset..reference.end.offset],
                "target"
            );
        }
        let spans = index
            .references_at(target.declaration.start.offset, true)
            .unwrap_or_else(|| panic!("target references missing"));
        assert_eq!(spans.len(), 5);
        assert!(
            spans
                .windows(2)
                .all(|pair| pair[0].start.offset < pair[1].start.offset)
        );
    }
}
