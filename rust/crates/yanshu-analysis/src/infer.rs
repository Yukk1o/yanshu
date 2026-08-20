#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_syntax::{
    Datum, DatumKind, Expression, ExpressionKind, Pattern, PatternKind, Program, SchemaKind,
};

use crate::Type;

pub(crate) fn infer_program(program: &Program) -> YanshuResult<BTreeMap<String, Type>> {
    Inferencer::new(program).infer(program)
}

struct Inferencer {
    next_variable: u32,
    substitutions: BTreeMap<u32, Type>,
    guest_bindings: BTreeSet<String>,
    text_library: bool,
}

impl Inferencer {
    fn new(program: &Program) -> Self {
        let mut guest_bindings = BTreeSet::new();
        guest_bindings.extend(program.schemas.iter().map(|schema| schema.name.clone()));
        guest_bindings.extend(
            program
                .data_types
                .iter()
                .flat_map(|data_type| data_type.variants.iter())
                .map(|variant| variant.name.clone()),
        );
        guest_bindings.extend(
            program
                .definitions
                .iter()
                .map(|definition| definition.name.clone()),
        );
        Self {
            next_variable: 0,
            substitutions: BTreeMap::new(),
            guest_bindings,
            text_library: program
                .libraries
                .iter()
                .any(|library| library.name == "text" && library.version == 1),
        }
    }

    fn infer(mut self, program: &Program) -> YanshuResult<BTreeMap<String, Type>> {
        let mut environment = BTreeMap::new();
        for schema in &program.schemas {
            environment.insert(
                schema.name.clone(),
                Type::Schema(Box::new(type_for_schema(&schema.kind))),
            );
        }
        for data_type in &program.data_types {
            for variant in &data_type.variants {
                let parameters = variant
                    .fields
                    .iter()
                    .map(|field| {
                        field
                            .type_expression
                            .as_ref()
                            .map_or_else(|| self.fresh(), Type::from_expression)
                    })
                    .collect();
                environment.insert(
                    variant.name.clone(),
                    Type::Function {
                        parameters,
                        result: Box::new(Type::User(data_type.name.clone())),
                    },
                );
            }
        }
        for definition in &program.definitions {
            let declared = program
                .signatures
                .iter()
                .find(|signature| signature.name == definition.name)
                .map(|signature| Type::Function {
                    parameters: signature
                        .parameters
                        .iter()
                        .map(Type::from_expression)
                        .collect(),
                    result: Box::new(Type::from_expression(&signature.result)),
                })
                .unwrap_or_else(|| self.fresh());
            environment.insert(definition.name.clone(), declared);
        }

        for definition in &program.definitions {
            let inferred =
                self.infer_expression(&definition.expression, &mut environment.clone())?;
            let declared = environment
                .get(&definition.name)
                .cloned()
                .ok_or_else(|| internal("definition type seed is missing"))?;
            self.unify(declared, inferred, definition.expression.span)?;
        }

        let mut result = BTreeMap::new();
        for (name, value) in environment {
            if self.guest_bindings.contains(&name) {
                result.insert(name, self.resolve(value));
            }
        }
        Ok(result)
    }

    fn fresh(&mut self) -> Type {
        let identifier = self.next_variable;
        self.next_variable = self.next_variable.saturating_add(1);
        Type::Variable(identifier)
    }

    fn infer_expression(
        &mut self,
        expression: &Expression,
        environment: &mut BTreeMap<String, Type>,
    ) -> YanshuResult<Type> {
        match &expression.kind {
            ExpressionKind::Literal(datum) | ExpressionKind::Quote(datum) => {
                self.infer_datum(datum)
            }
            ExpressionKind::Variable(name) => environment
                .get(name)
                .cloned()
                .or_else(|| self.primitive_value_type(name))
                .ok_or_else(|| {
                    Diagnostic::new(
                        "TYPE_UNBOUND_NAME",
                        "static analysis found an unbound name",
                        json!({ "name": name }),
                    )
                    .at(expression.span)
                }),
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => {
                self.infer_expression(condition, environment)?;
                let consequent = self.infer_expression(consequent, environment)?;
                let alternative = self.infer_expression(alternative, environment)?;
                self.unify(consequent.clone(), alternative, expression.span)?;
                Ok(self.resolve(consequent))
            }
            ExpressionKind::And(items) | ExpressionKind::Or(items) => {
                let Some(first) = items.first() else {
                    return Ok(Type::Boolean);
                };
                let inferred = self.infer_expression(first, environment)?;
                for item in &items[1..] {
                    let item_type = self.infer_expression(item, environment)?;
                    self.unify(inferred.clone(), item_type, item.span)?;
                }
                Ok(self.resolve(inferred))
            }
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => {
                let result = self.infer_expression(alternative, environment)?;
                for clause in clauses {
                    self.infer_expression(&clause.condition, environment)?;
                    let clause_type = self.infer_expression(&clause.expression, environment)?;
                    self.unify(result.clone(), clause_type, clause.expression.span)?;
                }
                Ok(self.resolve(result))
            }
            ExpressionKind::Match { value, arms } => {
                let value_type = self.infer_expression(value, environment)?;
                let result = self.fresh();
                for arm in arms {
                    let mut arm_environment = environment.clone();
                    self.infer_pattern(&arm.pattern, value_type.clone(), &mut arm_environment)?;
                    let arm_type = self.infer_expression(&arm.expression, &mut arm_environment)?;
                    self.unify(result.clone(), arm_type, arm.expression.span)?;
                }
                Ok(self.resolve(result))
            }
            ExpressionKind::Let { bindings, body } => {
                let mut local = environment.clone();
                for binding in bindings {
                    let value = self.infer_expression(&binding.expression, &mut local)?;
                    local.insert(binding.name.clone(), value);
                }
                self.infer_expression(body, &mut local)
            }
            ExpressionKind::Function { parameters, body } => {
                let mut local = environment.clone();
                let parameter_types = parameters
                    .iter()
                    .map(|parameter| {
                        let inferred = self.fresh();
                        local.insert(parameter.clone(), inferred.clone());
                        inferred
                    })
                    .collect::<Vec<_>>();
                let result = self.infer_expression(body, &mut local)?;
                Ok(Type::Function {
                    parameters: parameter_types
                        .into_iter()
                        .map(|value| self.resolve(value))
                        .collect(),
                    result: Box::new(self.resolve(result)),
                })
            }
            ExpressionKind::Do(items) => {
                let mut result = Type::Nil;
                for item in items {
                    result = self.infer_expression(item, environment)?;
                }
                Ok(result)
            }
            ExpressionKind::Call { callee, arguments } => {
                if let ExpressionKind::Variable(name) = &callee.kind
                    && !environment.contains_key(name)
                    && !self.guest_bindings.contains(name)
                    && self.is_known_primitive(name)
                {
                    return self.infer_primitive_call(
                        name,
                        arguments,
                        environment,
                        expression.span,
                    );
                }
                let callable = self.infer_expression(callee, environment)?;
                let argument_types = arguments
                    .iter()
                    .map(|argument| self.infer_expression(argument, environment))
                    .collect::<YanshuResult<Vec<_>>>()?;
                let result = self.fresh();
                self.unify(
                    callable,
                    Type::Function {
                        parameters: argument_types,
                        result: Box::new(result.clone()),
                    },
                    expression.span,
                )?;
                Ok(self.resolve(result))
            }
        }
    }

    fn infer_pattern(
        &mut self,
        pattern: &Pattern,
        expected: Type,
        environment: &mut BTreeMap<String, Type>,
    ) -> YanshuResult<()> {
        match &pattern.kind {
            PatternKind::Wildcard => Ok(()),
            PatternKind::Binding(name) => {
                environment.insert(name.clone(), self.resolve(expected));
                Ok(())
            }
            PatternKind::Literal(datum) => {
                let actual = self.infer_datum(datum)?;
                self.unify(expected, actual, pattern.span)
            }
            PatternKind::Variant { name, fields } => {
                let constructor = environment.get(name).cloned().ok_or_else(|| {
                    Diagnostic::new(
                        "TYPE_UNKNOWN_CONSTRUCTOR",
                        "pattern names an unknown data constructor",
                        json!({ "name": name }),
                    )
                    .at(pattern.span)
                })?;
                let Type::Function { parameters, result } = self.resolve(constructor) else {
                    return Err(Diagnostic::new(
                        "TYPE_PATTERN_NOT_CONSTRUCTOR",
                        "variant pattern name is not a data constructor",
                        json!({ "name": name }),
                    )
                    .at(pattern.span));
                };
                if parameters.len() != fields.len() {
                    return Err(arity_diagnostic(
                        name,
                        parameters.len(),
                        fields.len(),
                        pattern.span,
                    ));
                }
                self.unify(expected, *result, pattern.span)?;
                for (field, field_type) in fields.iter().zip(parameters) {
                    self.infer_pattern(field, field_type, environment)?;
                }
                Ok(())
            }
        }
    }

    fn infer_datum(&mut self, datum: &Datum) -> YanshuResult<Type> {
        match &datum.kind {
            DatumKind::Integer(_) => Ok(Type::Integer),
            DatumKind::Bool(_) => Ok(Type::Boolean),
            DatumKind::String(_) => Ok(Type::String),
            DatumKind::Symbol(_) => Ok(Type::Symbol),
            DatumKind::List(items) if items.is_empty() => Ok(Type::Nil),
            DatumKind::List(items) => {
                let item_type = self.infer_datum(&items[0])?;
                for item in &items[1..] {
                    let actual = self.infer_datum(item)?;
                    self.unify(item_type.clone(), actual, item.span)?;
                }
                Ok(Type::List(Box::new(self.resolve(item_type))))
            }
        }
    }

    fn infer_primitive_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        environment: &mut BTreeMap<String, Type>,
        span: Span,
    ) -> YanshuResult<Type> {
        let argument_types = arguments
            .iter()
            .map(|argument| self.infer_expression(argument, environment))
            .collect::<YanshuResult<Vec<_>>>()?;
        match name {
            "+" | "*" => {
                self.require_each(name, &argument_types, Type::Integer, span)?;
                Ok(Type::Integer)
            }
            "-" => {
                require_arity(name, arguments.len(), 1, None, span)?;
                self.require_each(name, &argument_types, Type::Integer, span)?;
                Ok(Type::Integer)
            }
            "quotient" | "remainder" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.require_each(name, &argument_types, Type::Integer, span)?;
                Ok(Type::Integer)
            }
            "checked-quotient" | "checked-remainder" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.require_each(name, &argument_types, Type::Integer, span)?;
                Ok(Type::Result {
                    success: Box::new(Type::Integer),
                    error: Box::new(Type::Map),
                })
            }
            "=" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                Ok(Type::Boolean)
            }
            "<" | "<=" | ">" | ">=" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.require_each(name, &argument_types, Type::Integer, span)?;
                Ok(Type::Boolean)
            }
            "not" | "integer?" | "boolean?" | "string?" | "list?" | "map?" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::Boolean)
            }
            "string-append" => {
                self.require_each(name, &argument_types, Type::String, span)?;
                Ok(Type::String)
            }
            "number->string" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                self.unify(Type::Integer, argument_types[0].clone(), span)?;
                Ok(Type::String)
            }
            "list" => {
                let item = self.fresh();
                for argument in argument_types {
                    self.unify(item.clone(), argument, span)?;
                }
                Ok(Type::List(Box::new(self.resolve(item))))
            }
            "list-map" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                let input = self.fresh();
                let output = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::Function {
                        parameters: vec![input.clone()],
                        result: Box::new(output.clone()),
                    },
                    span,
                )?;
                self.unify(argument_types[1].clone(), Type::List(Box::new(input)), span)?;
                Ok(Type::List(Box::new(self.resolve(output))))
            }
            "list-filter" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                let input = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::Function {
                        parameters: vec![input.clone()],
                        result: Box::new(Type::Any),
                    },
                    span,
                )?;
                self.unify(
                    argument_types[1].clone(),
                    Type::List(Box::new(input.clone())),
                    span,
                )?;
                Ok(Type::List(Box::new(self.resolve(input))))
            }
            "list-fold" => {
                require_arity(name, arguments.len(), 3, Some(3), span)?;
                let item = self.fresh();
                let accumulator = argument_types[1].clone();
                self.unify(
                    argument_types[0].clone(),
                    Type::Function {
                        parameters: vec![accumulator.clone(), item.clone()],
                        result: Box::new(accumulator.clone()),
                    },
                    span,
                )?;
                self.unify(argument_types[2].clone(), Type::List(Box::new(item)), span)?;
                Ok(self.resolve(accumulator))
            }
            "sum" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                self.unify(
                    argument_types[0].clone(),
                    Type::List(Box::new(Type::Integer)),
                    span,
                )?;
                Ok(Type::Integer)
            }
            "empty?" | "length" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let item = self.fresh();
                self.unify(argument_types[0].clone(), Type::List(Box::new(item)), span)?;
                Ok(if name == "empty?" {
                    Type::Boolean
                } else {
                    Type::Integer
                })
            }
            "first" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let item = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::List(Box::new(item.clone())),
                    span,
                )?;
                Ok(self.resolve(item))
            }
            "rest" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let item = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::List(Box::new(item.clone())),
                    span,
                )?;
                Ok(Type::List(Box::new(self.resolve(item))))
            }
            "map" => {
                if !arguments.len().is_multiple_of(2) {
                    return Err(arity_diagnostic(
                        name,
                        arguments.len() + 1,
                        arguments.len(),
                        span,
                    ));
                }
                Ok(Type::Map)
            }
            "get" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.unify(Type::Map, argument_types[0].clone(), span)?;
                Ok(Type::Any)
            }
            "get-or" => {
                require_arity(name, arguments.len(), 3, Some(3), span)?;
                self.unify(Type::Map, argument_types[0].clone(), span)?;
                Ok(Type::Any)
            }
            "has-key?" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.unify(Type::Map, argument_types[0].clone(), span)?;
                Ok(Type::Boolean)
            }
            "assoc" => {
                require_arity(name, arguments.len(), 3, Some(3), span)?;
                self.unify(Type::Map, argument_types[0].clone(), span)?;
                Ok(Type::Map)
            }
            "ok" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let error = self.fresh();
                Ok(Type::Result {
                    success: Box::new(argument_types[0].clone()),
                    error: Box::new(error),
                })
            }
            "err" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let success = self.fresh();
                Ok(Type::Result {
                    success: Box::new(success),
                    error: Box::new(argument_types[0].clone()),
                })
            }
            "ok?" | "err?" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::Boolean)
            }
            "result-value" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::Any)
            }
            "unwrap" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                let success = self.fresh();
                let error = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::Result {
                        success: Box::new(success.clone()),
                        error: Box::new(error),
                    },
                    span,
                )?;
                Ok(self.resolve(success))
            }
            "validate" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                let value = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::Schema(Box::new(value.clone())),
                    span,
                )?;
                Ok(Type::Result {
                    success: Box::new(self.resolve(value)),
                    error: Box::new(Type::List(Box::new(Type::Map))),
                })
            }
            "validate-report" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                let value = self.fresh();
                self.unify(
                    argument_types[0].clone(),
                    Type::Schema(Box::new(value)),
                    span,
                )?;
                Ok(Type::Map)
            }
            "api-response" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.unify(Type::Integer, argument_types[0].clone(), span)?;
                Ok(Type::Map)
            }
            "api-error" => {
                require_arity(name, arguments.len(), 3, Some(4), span)?;
                self.unify(Type::Integer, argument_types[0].clone(), span)?;
                self.unify(Type::String, argument_types[1].clone(), span)?;
                self.unify(Type::String, argument_types[2].clone(), span)?;
                Ok(Type::Map)
            }
            "log" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::Nil)
            }
            "now-ms" => {
                require_arity(name, arguments.len(), 0, Some(0), span)?;
                Ok(Type::Integer)
            }
            "kv-get" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                Ok(Type::Any)
            }
            "kv-put" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                Ok(Type::Nil)
            }
            "kv-delete" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::Boolean)
            }
            "kv-list" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                Ok(Type::List(Box::new(Type::Any)))
            }
            "text/length" => {
                require_arity(name, arguments.len(), 1, Some(1), span)?;
                self.unify(Type::String, argument_types[0].clone(), span)?;
                Ok(Type::Integer)
            }
            "text/starts-with?" | "text/ends-with?" | "text/contains?" => {
                require_arity(name, arguments.len(), 2, Some(2), span)?;
                self.require_each(name, &argument_types, Type::String, span)?;
                Ok(Type::Boolean)
            }
            "text/replace" => {
                require_arity(name, arguments.len(), 3, Some(3), span)?;
                self.require_each(name, &argument_types, Type::String, span)?;
                Ok(Type::String)
            }
            _ => Err(Diagnostic::new(
                "TYPE_UNKNOWN_PRIMITIVE",
                "static analysis has no contract for a primitive",
                json!({ "name": name }),
            )
            .at(span)),
        }
    }

    fn require_each(
        &mut self,
        _name: &str,
        actual: &[Type],
        expected: Type,
        span: Span,
    ) -> YanshuResult<()> {
        for value in actual {
            self.unify(expected.clone(), value.clone(), span)?;
        }
        Ok(())
    }

    fn primitive_value_type(&mut self, name: &str) -> Option<Type> {
        let unary = |parameter, result| Type::Function {
            parameters: vec![parameter],
            result: Box::new(result),
        };
        match name {
            "+" | "-" | "*" => Some(Type::Function {
                parameters: vec![Type::Integer, Type::Integer],
                result: Box::new(Type::Integer),
            }),
            "not" => Some(unary(Type::Any, Type::Boolean)),
            "number->string" => Some(unary(Type::Integer, Type::String)),
            _ => None,
        }
    }

    fn is_known_primitive(&self, name: &str) -> bool {
        const CORE: &[&str] = &[
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
        ];
        CORE.contains(&name)
            || (self.text_library
                && matches!(
                    name,
                    "text/length"
                        | "text/starts-with?"
                        | "text/ends-with?"
                        | "text/contains?"
                        | "text/replace"
                ))
    }

    fn resolve(&self, value: Type) -> Type {
        match value {
            Type::Variable(identifier) => self
                .substitutions
                .get(&identifier)
                .cloned()
                .map_or(Type::Variable(identifier), |value| self.resolve(value)),
            Type::List(item) => Type::List(Box::new(self.resolve(*item))),
            Type::Result { success, error } => Type::Result {
                success: Box::new(self.resolve(*success)),
                error: Box::new(self.resolve(*error)),
            },
            Type::Schema(item) => Type::Schema(Box::new(self.resolve(*item))),
            Type::Function { parameters, result } => Type::Function {
                parameters: parameters
                    .into_iter()
                    .map(|value| self.resolve(value))
                    .collect(),
                result: Box::new(self.resolve(*result)),
            },
            value => value,
        }
    }

    fn unify(&mut self, left: Type, right: Type, span: Span) -> YanshuResult<()> {
        let left = self.resolve(left);
        let right = self.resolve(right);
        match (left, right) {
            (Type::Any, _) | (_, Type::Any) => Ok(()),
            (Type::Variable(left), Type::Variable(right)) if left == right => Ok(()),
            (Type::Variable(identifier), value) | (value, Type::Variable(identifier)) => {
                if occurs(identifier, &value) {
                    return Err(Diagnostic::new(
                        "TYPE_RECURSIVE_UNIFICATION",
                        "type inference would construct an infinite type",
                        json!({ "variable": identifier, "type": value.display() }),
                    )
                    .at(span));
                }
                self.substitutions.insert(identifier, value);
                Ok(())
            }
            (Type::Nil, Type::List(_)) | (Type::List(_), Type::Nil) | (Type::Nil, Type::Nil) => {
                Ok(())
            }
            (Type::List(left), Type::List(right)) | (Type::Schema(left), Type::Schema(right)) => {
                self.unify(*left, *right, span)
            }
            (
                Type::Result {
                    success: left_success,
                    error: left_error,
                },
                Type::Result {
                    success: right_success,
                    error: right_error,
                },
            ) => {
                self.unify(*left_success, *right_success, span)?;
                self.unify(*left_error, *right_error, span)
            }
            (
                Type::Function {
                    parameters: left_parameters,
                    result: left_result,
                },
                Type::Function {
                    parameters: right_parameters,
                    result: right_result,
                },
            ) => {
                if left_parameters.len() != right_parameters.len() {
                    return Err(arity_diagnostic(
                        "function type",
                        left_parameters.len(),
                        right_parameters.len(),
                        span,
                    ));
                }
                for (left, right) in left_parameters.into_iter().zip(right_parameters) {
                    self.unify(left, right, span)?;
                }
                self.unify(*left_result, *right_result, span)
            }
            (left, right) if left == right => Ok(()),
            (left, right) => Err(Diagnostic::new(
                "TYPE_MISMATCH",
                "static types do not unify",
                json!({ "left": left.display(), "right": right.display() }),
            )
            .at(span)),
        }
    }
}

fn occurs(identifier: u32, value: &Type) -> bool {
    match value {
        Type::Variable(candidate) => identifier == *candidate,
        Type::List(item) | Type::Schema(item) => occurs(identifier, item),
        Type::Result { success, error } => occurs(identifier, success) || occurs(identifier, error),
        Type::Function { parameters, result } => {
            parameters.iter().any(|value| occurs(identifier, value)) || occurs(identifier, result)
        }
        Type::Any
        | Type::Integer
        | Type::Boolean
        | Type::String
        | Type::Symbol
        | Type::Nil
        | Type::Map
        | Type::User(_) => false,
    }
}

fn type_for_schema(schema: &SchemaKind) -> Type {
    match schema {
        SchemaKind::Any => Type::Any,
        SchemaKind::Enum { values } => {
            let mut kinds = values.iter().map(|value| match value.kind {
                DatumKind::Integer(_) => Type::Integer,
                DatumKind::Bool(_) => Type::Boolean,
                DatumKind::String(_) => Type::String,
                DatumKind::Symbol(_) => Type::Symbol,
                DatumKind::List(_) => Type::Any,
            });
            let first = kinds.next().unwrap_or(Type::Any);
            if kinds.all(|item| item == first) {
                first
            } else {
                Type::Any
            }
        }
        SchemaKind::Union { variants } => {
            let mut kinds = variants.iter().map(type_for_schema);
            let first = kinds.next().unwrap_or(Type::Any);
            if kinds.all(|item| item == first) {
                first
            } else {
                Type::Any
            }
        }
        SchemaKind::String { .. } => Type::String,
        SchemaKind::Integer { .. } => Type::Integer,
        SchemaKind::Boolean => Type::Boolean,
        SchemaKind::List { item, .. } => Type::List(Box::new(type_for_schema(item))),
        SchemaKind::Object { .. } => Type::Map,
    }
}

fn require_arity(
    name: &str,
    actual: usize,
    minimum: usize,
    maximum: Option<usize>,
    span: Span,
) -> YanshuResult<()> {
    if actual >= minimum && maximum.is_none_or(|maximum| actual <= maximum) {
        Ok(())
    } else {
        Err(Diagnostic::new(
            "TYPE_ARITY",
            "static call arity does not match its contract",
            json!({ "name": name, "minimum": minimum, "maximum": maximum, "actual": actual }),
        )
        .at(span))
    }
}

fn arity_diagnostic(name: &str, expected: usize, actual: usize, span: Span) -> Diagnostic {
    Diagnostic::new(
        "TYPE_ARITY",
        "static call arity does not match its contract",
        json!({ "name": name, "minimum": expected, "maximum": expected, "actual": actual }),
    )
    .at(span)
}

fn internal(message: &'static str) -> Diagnostic {
    Diagnostic::simple("TYPE_INTERNAL", message)
}
