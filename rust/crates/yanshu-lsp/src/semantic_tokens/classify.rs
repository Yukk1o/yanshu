use std::collections::{BTreeMap, BTreeSet};

use yanshu_diagnostic::{Span, YanshuResult};
use yanshu_library::trusted_contract;
use yanshu_syntax::{
    Datum, DatumKind, ExpressionKind, Program, ReaderLimits, SymbolBindingKind, expression_nodes,
    read_source, symbol_index,
};

use crate::hover::catalog::{FormContext, form_help, primitive_help};

use super::{
    MAXIMUM_SEMANTIC_TOKENS, MODIFIER_DECLARATION, MODIFIER_DEFAULT_LIBRARY, MODIFIER_DEFINITION,
    MODIFIER_READONLY, PRIORITY_BINDING, PRIORITY_LIBRARY, PRIORITY_STRUCTURE, SemanticToken,
    SpanKey, TokenType, span_key, span_mismatch, token_limit,
};

pub(super) fn semantic_token_candidates(program: &Program) -> YanshuResult<Vec<SemanticToken>> {
    let root = read_source(&program.source, ReaderLimits::default())?;
    let nodes = expression_nodes(program)
        .into_iter()
        .map(|node| span_key(node.span))
        .collect::<BTreeSet<_>>();
    let constructors = program
        .data_types
        .iter()
        .flat_map(|data_type| data_type.variants.iter())
        .map(|variant| variant.name.as_str())
        .collect::<BTreeSet<_>>();
    let schemas = program
        .schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut collector = TokenCollector {
        source: &program.source,
        tokens: BTreeMap::new(),
    };

    collect_structure(&root, &constructors, &schemas, &mut collector)?;
    collect_bindings(program, &mut collector)?;
    collect_expressions(
        program,
        &root,
        &nodes,
        &constructors,
        &schemas,
        false,
        &mut collector,
    )?;

    let tokens = collector.tokens.into_values().collect::<Vec<_>>();
    if tokens.len() > MAXIMUM_SEMANTIC_TOKENS {
        return Err(token_limit(tokens.len()));
    }
    Ok(tokens)
}

struct TokenCollector<'source> {
    source: &'source str,
    tokens: BTreeMap<SpanKey, SemanticToken>,
}

impl TokenCollector<'_> {
    fn add_symbol(
        &mut self,
        datum: &Datum,
        token_type: TokenType,
        modifiers: u32,
        priority: u8,
    ) -> YanshuResult<()> {
        let Some(name) = datum.symbol() else {
            return Err(span_mismatch());
        };
        self.add_named_span(name, datum.span, token_type, modifiers, priority)
    }

    fn add_keyword(&mut self, datum: &Datum) -> YanshuResult<()> {
        let Some(name) = datum.symbol() else {
            return Err(span_mismatch());
        };
        let source_token = self
            .source
            .get(datum.span.start.offset..datum.span.end.offset)
            .ok_or_else(span_mismatch)?;
        if source_token != name && !(name == "quote" && source_token == "'") {
            return Err(span_mismatch());
        }
        self.insert(datum.span, TokenType::Keyword, 0, PRIORITY_STRUCTURE)
    }

    fn add_named_span(
        &mut self,
        name: &str,
        span: Span,
        token_type: TokenType,
        modifiers: u32,
        priority: u8,
    ) -> YanshuResult<()> {
        if self.source.get(span.start.offset..span.end.offset) != Some(name) {
            return Err(span_mismatch());
        }
        self.insert(span, token_type, modifiers, priority)
    }

    fn insert(
        &mut self,
        span: Span,
        token_type: TokenType,
        modifiers: u32,
        priority: u8,
    ) -> YanshuResult<()> {
        if span.start.offset >= span.end.offset {
            return Err(span_mismatch());
        }
        let key = span_key(span);
        let candidate = SemanticToken {
            span,
            token_type,
            modifiers,
            priority,
        };
        if self
            .tokens
            .get(&key)
            .is_none_or(|existing| existing.priority <= priority)
        {
            self.tokens.insert(key, candidate);
        }
        if self.tokens.len() > MAXIMUM_SEMANTIC_TOKENS {
            return Err(token_limit(self.tokens.len()));
        }
        Ok(())
    }
}

fn collect_structure(
    root: &Datum,
    constructors: &BTreeSet<&str>,
    schemas: &BTreeSet<&str>,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    let form = root.list().ok_or_else(span_mismatch)?;
    collector.add_keyword(form.first().ok_or_else(span_mismatch)?)?;

    for member in form.iter().skip(1) {
        let parts = member.list().ok_or_else(span_mismatch)?;
        let member_head = parts.first().ok_or_else(span_mismatch)?;
        let member_name = member_head.symbol().ok_or_else(span_mismatch)?;
        if form_help(FormContext::TopLevel, member_name).is_some() {
            collector.add_keyword(member_head)?;
        }
        match member_name {
            "name" => add_indexed_symbol(
                parts,
                1,
                TokenType::Namespace,
                MODIFIER_DECLARATION | MODIFIER_READONLY,
                collector,
            )?,
            "imports" => {
                for imported in parts.iter().skip(1) {
                    collector.add_symbol(
                        imported,
                        TokenType::Namespace,
                        MODIFIER_READONLY,
                        PRIORITY_STRUCTURE,
                    )?;
                }
            }
            "libraries" => {
                for requirement in parts.iter().skip(1) {
                    let requirement = requirement.list().ok_or_else(span_mismatch)?;
                    add_indexed_symbol(
                        requirement,
                        0,
                        TokenType::Namespace,
                        MODIFIER_READONLY | MODIFIER_DEFAULT_LIBRARY,
                        collector,
                    )?;
                }
            }
            "data" => collect_data(parts, collector)?,
            "export-types" => {
                for type_name in parts.iter().skip(1) {
                    collector.add_symbol(
                        type_name,
                        TokenType::Type,
                        MODIFIER_READONLY,
                        PRIORITY_STRUCTURE,
                    )?;
                }
            }
            "signature" => collect_type_at(parts, 2, collector)?,
            "schema" => {
                add_indexed_symbol(
                    parts,
                    1,
                    TokenType::Variable,
                    MODIFIER_DEFINITION | MODIFIER_READONLY,
                    collector,
                )?;
                collect_schema(parts.get(2).ok_or_else(span_mismatch)?, collector)?;
            }
            "export" => {
                for exported in parts.iter().skip(1) {
                    let name = exported.symbol().ok_or_else(span_mismatch)?;
                    if constructors.contains(name) {
                        collector.add_symbol(
                            exported,
                            TokenType::EnumMember,
                            MODIFIER_READONLY,
                            PRIORITY_STRUCTURE,
                        )?;
                    } else if schemas.contains(name) {
                        collector.add_symbol(
                            exported,
                            TokenType::Variable,
                            MODIFIER_READONLY,
                            PRIORITY_STRUCTURE,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_data(parts: &[Datum], collector: &mut TokenCollector<'_>) -> YanshuResult<()> {
    add_indexed_symbol(
        parts,
        1,
        TokenType::Type,
        MODIFIER_DEFINITION | MODIFIER_READONLY,
        collector,
    )?;
    for variant in parts.iter().skip(2) {
        let fields = variant.list().ok_or_else(span_mismatch)?;
        add_indexed_symbol(
            fields,
            0,
            TokenType::EnumMember,
            MODIFIER_DEFINITION | MODIFIER_READONLY,
            collector,
        )?;
        for field in fields.iter().skip(1) {
            if field.symbol().is_some() {
                collector.add_symbol(
                    field,
                    TokenType::Property,
                    MODIFIER_DECLARATION | MODIFIER_READONLY,
                    PRIORITY_STRUCTURE,
                )?;
                continue;
            }
            let field = field.list().ok_or_else(span_mismatch)?;
            add_indexed_symbol(
                field,
                0,
                TokenType::Property,
                MODIFIER_DECLARATION | MODIFIER_READONLY,
                collector,
            )?;
            collect_type_at(field, 1, collector)?;
        }
    }
    Ok(())
}

fn collect_type_at(
    parts: &[Datum],
    index: usize,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    collect_type(parts.get(index).ok_or_else(span_mismatch)?, collector)
}

fn collect_type(datum: &Datum, collector: &mut TokenCollector<'_>) -> YanshuResult<()> {
    if datum.symbol().is_some() {
        return collector.add_symbol(
            datum,
            TokenType::Type,
            MODIFIER_READONLY,
            PRIORITY_STRUCTURE,
        );
    }
    let form = datum.list().ok_or_else(span_mismatch)?;
    let head = form.first().ok_or_else(span_mismatch)?;
    let name = head.symbol().ok_or_else(span_mismatch)?;
    if form_help(FormContext::Type, name).is_none() {
        return Err(span_mismatch());
    }
    collector.add_keyword(head)?;
    match name {
        "list" => collect_type_at(form, 1, collector),
        "result" => {
            collect_type_at(form, 1, collector)?;
            collect_type_at(form, 2, collector)
        }
        "fn" => {
            let parameters = form
                .get(1)
                .and_then(Datum::list)
                .ok_or_else(span_mismatch)?;
            for parameter in parameters {
                collect_type(parameter, collector)?;
            }
            collect_type_at(form, 2, collector)
        }
        _ => Err(span_mismatch()),
    }
}

fn collect_schema(datum: &Datum, collector: &mut TokenCollector<'_>) -> YanshuResult<()> {
    if datum.symbol().is_some() {
        return collector.add_symbol(
            datum,
            TokenType::Type,
            MODIFIER_READONLY,
            PRIORITY_STRUCTURE,
        );
    }
    let form = datum.list().ok_or_else(span_mismatch)?;
    let head = form.first().ok_or_else(span_mismatch)?;
    let name = head.symbol().ok_or_else(span_mismatch)?;
    if form_help(FormContext::Schema, name).is_none() {
        return Err(span_mismatch());
    }
    collector.add_keyword(head)?;
    match name {
        "union" => {
            for variant in form.iter().skip(1) {
                collect_schema(variant, collector)?;
            }
        }
        "list" => collect_schema(form.get(1).ok_or_else(span_mismatch)?, collector)?,
        "object" => {
            for field in form.iter().skip(1) {
                let field = field.list().ok_or_else(span_mismatch)?;
                let field_head = field.first().ok_or_else(span_mismatch)?;
                let field_name = field_head.symbol().ok_or_else(span_mismatch)?;
                if form_help(FormContext::Schema, field_name).is_none() {
                    return Err(span_mismatch());
                }
                collector.add_keyword(field_head)?;
                collect_schema(field.get(2).ok_or_else(span_mismatch)?, collector)?;
            }
        }
        "enum" | "string" | "integer" => {}
        _ => return Err(span_mismatch()),
    }
    Ok(())
}

fn add_indexed_symbol(
    parts: &[Datum],
    index: usize,
    token_type: TokenType,
    modifiers: u32,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    collector.add_symbol(
        parts.get(index).ok_or_else(span_mismatch)?,
        token_type,
        modifiers,
        PRIORITY_STRUCTURE,
    )
}

fn collect_bindings(program: &Program, collector: &mut TokenCollector<'_>) -> YanshuResult<()> {
    let function_definitions = program
        .definitions
        .iter()
        .filter(|definition| matches!(definition.expression.kind, ExpressionKind::Function { .. }))
        .map(|definition| definition.name.as_str())
        .collect::<BTreeSet<_>>();
    let index = symbol_index(program)?;
    for binding in index.bindings() {
        let token_type = match binding.kind {
            SymbolBindingKind::Definition
                if function_definitions.contains(binding.name.as_str()) =>
            {
                TokenType::Function
            }
            SymbolBindingKind::Definition | SymbolBindingKind::Let | SymbolBindingKind::Pattern => {
                TokenType::Variable
            }
            SymbolBindingKind::Parameter => TokenType::Parameter,
        };
        let declaration_modifier = match binding.kind {
            SymbolBindingKind::Definition => MODIFIER_DEFINITION,
            SymbolBindingKind::Parameter | SymbolBindingKind::Let | SymbolBindingKind::Pattern => {
                MODIFIER_DECLARATION
            }
        };
        collector.add_named_span(
            &binding.name,
            binding.declaration,
            token_type,
            declaration_modifier | MODIFIER_READONLY,
            PRIORITY_BINDING,
        )?;
        for reference in &binding.references {
            collector.add_named_span(
                &binding.name,
                *reference,
                token_type,
                MODIFIER_READONLY,
                PRIORITY_BINDING,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_expressions(
    program: &Program,
    datum: &Datum,
    expression_spans: &BTreeSet<SpanKey>,
    constructors: &BTreeSet<&str>,
    schemas: &BTreeSet<&str>,
    is_call_head: bool,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    match &datum.kind {
        DatumKind::Symbol(name) => {
            if expression_spans.contains(&span_key(datum.span)) {
                collect_expression_symbol(
                    program,
                    datum,
                    name,
                    constructors,
                    schemas,
                    is_call_head,
                    collector,
                )?;
            }
        }
        DatumKind::List(items) => {
            let expression = expression_spans.contains(&span_key(datum.span));
            let expression_form = expression
                .then(|| items.first().and_then(Datum::symbol))
                .flatten()
                .and_then(|name| form_help(FormContext::Expression, name));
            if let Some(entry) = expression_form {
                collector.add_keyword(items.first().ok_or_else(span_mismatch)?)?;
                if entry.name == "cond" {
                    collect_cond_else(items, collector)?;
                }
                if entry.name == "match" {
                    collect_match_patterns(items, constructors, collector)?;
                }
                if entry.name == "quote" {
                    return Ok(());
                }
            }
            for (index, child) in items.iter().enumerate() {
                let child_is_call_head = expression && expression_form.is_none() && index == 0;
                collect_expressions(
                    program,
                    child,
                    expression_spans,
                    constructors,
                    schemas,
                    child_is_call_head,
                    collector,
                )?;
            }
        }
        DatumKind::Integer(_) | DatumKind::Bool(_) | DatumKind::String(_) => {}
    }
    Ok(())
}

fn collect_expression_symbol(
    program: &Program,
    datum: &Datum,
    name: &str,
    constructors: &BTreeSet<&str>,
    schemas: &BTreeSet<&str>,
    is_call_head: bool,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    if primitive_help(name).is_some() {
        let token_type = if is_symbolic_operator(name) {
            TokenType::Operator
        } else {
            TokenType::Function
        };
        return collector.add_symbol(
            datum,
            token_type,
            MODIFIER_DEFAULT_LIBRARY,
            PRIORITY_LIBRARY,
        );
    }
    if is_library_operation(program, name) {
        return collector.add_symbol(
            datum,
            TokenType::Function,
            MODIFIER_DEFAULT_LIBRARY,
            PRIORITY_LIBRARY,
        );
    }
    if constructors.contains(name) {
        return collector.add_symbol(
            datum,
            TokenType::EnumMember,
            MODIFIER_READONLY,
            PRIORITY_STRUCTURE,
        );
    }
    if schemas.contains(name) {
        return collector.add_symbol(
            datum,
            TokenType::Variable,
            MODIFIER_READONLY,
            PRIORITY_STRUCTURE,
        );
    }
    if is_call_head
        && name
            .split_once('/')
            .is_some_and(|(module, _)| program.imports.iter().any(|imported| imported == module))
    {
        return collector.add_symbol(
            datum,
            TokenType::Function,
            MODIFIER_READONLY,
            PRIORITY_STRUCTURE,
        );
    }
    Ok(())
}

fn collect_cond_else(items: &[Datum], collector: &mut TokenCollector<'_>) -> YanshuResult<()> {
    let alternative = items
        .last()
        .and_then(Datum::list)
        .ok_or_else(span_mismatch)?;
    collector.add_keyword(alternative.first().ok_or_else(span_mismatch)?)
}

fn collect_match_patterns(
    items: &[Datum],
    constructors: &BTreeSet<&str>,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    for arm in items.iter().skip(2) {
        let arm = arm.list().ok_or_else(span_mismatch)?;
        collect_pattern(
            arm.first().ok_or_else(span_mismatch)?,
            constructors,
            collector,
        )?;
    }
    Ok(())
}

fn collect_pattern(
    datum: &Datum,
    constructors: &BTreeSet<&str>,
    collector: &mut TokenCollector<'_>,
) -> YanshuResult<()> {
    if datum.symbol() == Some("_") {
        return collector.add_keyword(datum);
    }
    let Some(items) = datum.list() else {
        return Ok(());
    };
    let head = items.first().ok_or_else(span_mismatch)?;
    let name = head.symbol().ok_or_else(span_mismatch)?;
    if !constructors.contains(name) {
        return Err(span_mismatch());
    }
    collector.add_symbol(
        head,
        TokenType::EnumMember,
        MODIFIER_READONLY,
        PRIORITY_STRUCTURE,
    )?;
    for child in items.iter().skip(1) {
        collect_pattern(child, constructors, collector)?;
    }
    Ok(())
}

fn is_symbolic_operator(name: &str) -> bool {
    matches!(name, "+" | "-" | "*" | "=" | "<" | "<=" | ">" | ">=")
}

fn is_library_operation(program: &Program, public_name: &str) -> bool {
    let Some((library, operation)) = public_name.split_once('/') else {
        return false;
    };
    let version = program
        .libraries
        .iter()
        .find(|requirement| requirement.name == library)
        .map_or(1, |requirement| requirement.version);
    trusted_contract(library, version)
        .and_then(|contract| contract.operation(operation))
        .is_some()
}
