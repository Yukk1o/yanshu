#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::{Expression, ExpressionKind, Program};
use serde_json::json;

use crate::{AnalysisReport, DefinitionAnalysis, Type};

type Capabilities = BTreeSet<String>;

#[derive(Debug, Clone)]
enum Callable {
    Definition(String),
    Inline {
        parameters: Vec<String>,
        body: Expression,
        lexical: BTreeSet<String>,
        callables: BTreeMap<String, Callable>,
    },
    Primitive(String),
    Constructor,
    UnknownParameter(String),
}

pub(crate) fn analyze_program_effects(
    program: &Program,
    inferred: BTreeMap<String, Type>,
) -> AilResult<AnalysisReport> {
    let mut analyzer = EffectAnalyzer::new(program);
    let mut exports = BTreeMap::new();
    let mut closure = Capabilities::new();
    for export in &program.exports {
        if !analyzer.definitions.contains_key(export) {
            continue;
        }
        let effects = analyzer.analyze_definition(export, Vec::new(), &mut Vec::new())?;
        closure.extend(effects.iter().cloned());
        exports.insert(export.clone(), effects.into_iter().collect());
    }

    let declared = program
        .capabilities
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing = closure.difference(&declared).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::new(
            "EFFECT_CAPABILITY_NOT_DECLARED",
            "static capability closure exceeds the program declaration",
            json!({
                "missing": missing,
                "declared": declared,
                "computed": closure,
            }),
        ));
    }
    let unused = declared.difference(&closure).cloned().collect::<Vec<_>>();
    let definitions = program
        .definitions
        .iter()
        .map(|definition| DefinitionAnalysis {
            name: definition.name.clone(),
            inferred_type: inferred.get(&definition.name).cloned().unwrap_or(Type::Any),
            capabilities: analyzer
                .observed
                .get(&definition.name)
                .map_or_else(Vec::new, |effects| effects.iter().cloned().collect()),
        })
        .collect();
    Ok(AnalysisReport {
        definitions,
        exports,
        capability_closure: closure.into_iter().collect(),
        declared_capabilities: declared.into_iter().collect(),
        unused_capabilities: unused,
    })
}

struct EffectAnalyzer<'program> {
    definitions: BTreeMap<String, &'program Expression>,
    constructors: BTreeSet<String>,
    observed: BTreeMap<String, Capabilities>,
}

impl<'program> EffectAnalyzer<'program> {
    fn new(program: &'program Program) -> Self {
        Self {
            definitions: program
                .definitions
                .iter()
                .map(|definition| (definition.name.clone(), &definition.expression))
                .collect(),
            constructors: program
                .data_types
                .iter()
                .flat_map(|data_type| data_type.variants.iter())
                .map(|variant| variant.name.clone())
                .collect(),
            observed: BTreeMap::new(),
        }
    }

    fn analyze_definition(
        &mut self,
        name: &str,
        actual_callables: Vec<Option<Callable>>,
        stack: &mut Vec<String>,
    ) -> AilResult<Capabilities> {
        if stack.iter().any(|entry| entry == name) {
            return Ok(Capabilities::new());
        }
        let expression = self.definitions.get(name).copied().ok_or_else(|| {
            Diagnostic::new(
                "EFFECT_UNKNOWN_DEFINITION",
                "effect analysis cannot resolve a definition",
                json!({ "name": name }),
            )
        })?;
        stack.push(name.to_owned());
        let result = match &expression.kind {
            ExpressionKind::Function { parameters, body } => self.analyze_function_body(
                parameters,
                body,
                actual_callables,
                BTreeSet::new(),
                BTreeMap::new(),
                stack,
            ),
            _ => {
                let mut lexical = BTreeSet::new();
                let mut callables = BTreeMap::new();
                if let Some(callable) = self.resolve_callable(expression, &lexical, &callables) {
                    self.apply_callable(callable, &[], &lexical, &callables, stack, expression)
                } else {
                    self.analyze_expression(expression, &mut lexical, &mut callables, stack)
                }
            }
        };
        stack.pop();
        if let Ok(effects) = &result {
            self.observed
                .entry(name.to_owned())
                .or_default()
                .extend(effects.iter().cloned());
        }
        result
    }

    fn analyze_function_body(
        &mut self,
        parameters: &[String],
        body: &Expression,
        actual_callables: Vec<Option<Callable>>,
        mut lexical: BTreeSet<String>,
        mut callables: BTreeMap<String, Callable>,
        stack: &mut Vec<String>,
    ) -> AilResult<Capabilities> {
        for (index, parameter) in parameters.iter().enumerate() {
            lexical.insert(parameter.clone());
            if let Some(callable) = actual_callables.get(index).cloned().flatten() {
                callables.insert(parameter.clone(), callable);
            } else {
                callables.remove(parameter);
            }
        }
        self.analyze_expression(body, &mut lexical, &mut callables, stack)
    }

    fn analyze_expression(
        &mut self,
        expression: &Expression,
        lexical: &mut BTreeSet<String>,
        callables: &mut BTreeMap<String, Callable>,
        stack: &mut Vec<String>,
    ) -> AilResult<Capabilities> {
        let mut effects = Capabilities::new();
        match &expression.kind {
            ExpressionKind::Literal(_)
            | ExpressionKind::Variable(_)
            | ExpressionKind::Quote(_)
            | ExpressionKind::Function { .. } => {}
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => {
                self.extend_expression(&mut effects, condition, lexical, callables, stack)?;
                self.extend_expression(&mut effects, consequent, lexical, callables, stack)?;
                self.extend_expression(&mut effects, alternative, lexical, callables, stack)?;
            }
            ExpressionKind::And(items) | ExpressionKind::Or(items) | ExpressionKind::Do(items) => {
                for item in items {
                    self.extend_expression(&mut effects, item, lexical, callables, stack)?;
                }
            }
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => {
                for clause in clauses {
                    self.extend_expression(
                        &mut effects,
                        &clause.condition,
                        lexical,
                        callables,
                        stack,
                    )?;
                    self.extend_expression(
                        &mut effects,
                        &clause.expression,
                        lexical,
                        callables,
                        stack,
                    )?;
                }
                self.extend_expression(&mut effects, alternative, lexical, callables, stack)?;
            }
            ExpressionKind::Match { value, arms } => {
                self.extend_expression(&mut effects, value, lexical, callables, stack)?;
                for arm in arms {
                    let mut arm_lexical = lexical.clone();
                    collect_pattern_bindings(&arm.pattern, &mut arm_lexical);
                    let mut arm_callables = callables.clone();
                    self.extend_expression(
                        &mut effects,
                        &arm.expression,
                        &mut arm_lexical,
                        &mut arm_callables,
                        stack,
                    )?;
                }
            }
            ExpressionKind::Let { bindings, body } => {
                let mut local_lexical = lexical.clone();
                let mut local_callables = callables.clone();
                for binding in bindings {
                    self.extend_expression(
                        &mut effects,
                        &binding.expression,
                        &mut local_lexical,
                        &mut local_callables,
                        stack,
                    )?;
                    local_lexical.insert(binding.name.clone());
                    if let Some(callable) =
                        self.resolve_callable(&binding.expression, &local_lexical, &local_callables)
                    {
                        local_callables.insert(binding.name.clone(), callable);
                    } else {
                        local_callables.remove(&binding.name);
                    }
                }
                self.extend_expression(
                    &mut effects,
                    body,
                    &mut local_lexical,
                    &mut local_callables,
                    stack,
                )?;
            }
            ExpressionKind::Call { callee, arguments } => {
                self.extend_expression(&mut effects, callee, lexical, callables, stack)?;
                for argument in arguments {
                    self.extend_expression(&mut effects, argument, lexical, callables, stack)?;
                }
                let callable = self
                    .resolve_callable(callee, lexical, callables)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            "EFFECT_UNKNOWN_CALL_TARGET",
                            "effect analysis cannot resolve a call target",
                            json!({ "expression": callee.to_json() }),
                        )
                        .at(callee.span)
                    })?;
                effects.extend(
                    self.apply_callable(callable, arguments, lexical, callables, stack, callee)?,
                );
            }
        }
        Ok(effects)
    }

    fn extend_expression(
        &mut self,
        target: &mut Capabilities,
        expression: &Expression,
        lexical: &mut BTreeSet<String>,
        callables: &mut BTreeMap<String, Callable>,
        stack: &mut Vec<String>,
    ) -> AilResult<()> {
        target.extend(self.analyze_expression(expression, lexical, callables, stack)?);
        Ok(())
    }

    fn apply_callable(
        &mut self,
        callable: Callable,
        arguments: &[Expression],
        lexical: &BTreeSet<String>,
        callables: &BTreeMap<String, Callable>,
        stack: &mut Vec<String>,
        source: &Expression,
    ) -> AilResult<Capabilities> {
        match callable {
            Callable::Definition(name) => {
                let actual = arguments
                    .iter()
                    .map(|argument| self.resolve_callable(argument, lexical, callables))
                    .collect();
                self.analyze_definition(&name, actual, stack)
            }
            Callable::Inline {
                parameters,
                body,
                lexical: captured_lexical,
                callables: captured_callables,
            } => {
                let actual = arguments
                    .iter()
                    .map(|argument| self.resolve_callable(argument, lexical, callables))
                    .collect();
                self.analyze_function_body(
                    &parameters,
                    &body,
                    actual,
                    captured_lexical,
                    captured_callables,
                    stack,
                )
            }
            Callable::Primitive(name) => {
                let mut effects = capability_for_primitive(&name)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Capabilities>();
                if matches!(name.as_str(), "list-map" | "list-filter" | "list-fold") {
                    let callback = arguments
                        .first()
                        .and_then(|argument| self.resolve_callable(argument, lexical, callables))
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "EFFECT_UNKNOWN_CALLBACK",
                                "effect analysis cannot resolve a higher-order callback",
                                json!({ "primitive": name }),
                            )
                            .at(source.span)
                        })?;
                    effects.extend(self.apply_callable(
                        callback,
                        &[],
                        lexical,
                        callables,
                        stack,
                        source,
                    )?);
                }
                Ok(effects)
            }
            Callable::Constructor => Ok(Capabilities::new()),
            Callable::UnknownParameter(name) => Err(Diagnostic::new(
                "EFFECT_UNRESOLVED_PARAMETER",
                "exported capability closure depends on an unresolved function parameter",
                json!({ "parameter": name }),
            )
            .at(source.span)),
        }
    }

    fn resolve_callable(
        &self,
        expression: &Expression,
        lexical: &BTreeSet<String>,
        callables: &BTreeMap<String, Callable>,
    ) -> Option<Callable> {
        match &expression.kind {
            ExpressionKind::Variable(name) => {
                if let Some(callable) = callables.get(name) {
                    return Some(callable.clone());
                }
                if lexical.contains(name) {
                    return Some(Callable::UnknownParameter(name.clone()));
                }
                if self.definitions.contains_key(name) {
                    return Some(Callable::Definition(name.clone()));
                }
                if self.constructors.contains(name) {
                    return Some(Callable::Constructor);
                }
                if is_primitive(name) {
                    return Some(Callable::Primitive(name.clone()));
                }
                None
            }
            ExpressionKind::Function { parameters, body } => Some(Callable::Inline {
                parameters: parameters.clone(),
                body: body.as_ref().clone(),
                lexical: lexical.clone(),
                callables: callables.clone(),
            }),
            _ => None,
        }
    }
}

fn capability_for_primitive(name: &str) -> Option<&'static str> {
    match name {
        "log" => Some("log"),
        "now-ms" => Some("clock"),
        "kv-get" | "kv-put" | "kv-delete" | "kv-list" => Some("kv"),
        _ => None,
    }
}

fn is_primitive(name: &str) -> bool {
    const PRIMITIVES: &[&str] = &[
        "+",
        "-",
        "*",
        "quotient",
        "remainder",
        "checked-quotient",
        "checked-remainder",
        "=",
        "<",
        "<=",
        ">",
        ">=",
        "not",
        "integer?",
        "boolean?",
        "string?",
        "list?",
        "map?",
        "string-append",
        "number->string",
        "list",
        "list-map",
        "list-filter",
        "list-fold",
        "sum",
        "empty?",
        "length",
        "first",
        "rest",
        "map",
        "get",
        "get-or",
        "has-key?",
        "assoc",
        "ok",
        "err",
        "ok?",
        "err?",
        "result-value",
        "unwrap",
        "validate",
        "validate-report",
        "api-response",
        "api-error",
        "log",
        "now-ms",
        "kv-get",
        "kv-put",
        "kv-delete",
        "kv-list",
        "text/length",
        "text/starts-with?",
        "text/ends-with?",
        "text/contains?",
        "text/replace",
    ];
    PRIMITIVES.contains(&name)
}

fn collect_pattern_bindings(pattern: &ail_syntax::Pattern, bindings: &mut BTreeSet<String>) {
    match &pattern.kind {
        ail_syntax::PatternKind::Binding(name) => {
            bindings.insert(name.clone());
        }
        ail_syntax::PatternKind::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(field, bindings);
            }
        }
        ail_syntax::PatternKind::Wildcard | ail_syntax::PatternKind::Literal(_) => {}
    }
}
