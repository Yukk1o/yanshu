#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ail_syntax::{Expression, ExpressionKind, Pattern, PatternKind, Program};
use serde_json::{Value, json};

use crate::{AnalysisReport, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNode {
    pub id: String,
    pub source: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub inferred_type: Type,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDocument {
    pub renderer: &'static str,
    pub editable: bool,
    pub text: String,
    pub nodes: Vec<ReviewNode>,
}

impl ReviewDocument {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "renderer": self.renderer,
            "editable": self.editable,
            "text": self.text,
            "nodes": self.nodes.iter().map(ReviewNode::to_json).collect::<Vec<_>>(),
        })
    }
}

impl ReviewNode {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "source": self.source,
            "span": {
                "start": { "line": self.start_line, "column": self.start_column },
                "end": { "line": self.end_line, "column": self.end_column },
            },
            "type": self.inferred_type.to_json(),
            "capabilities": self.capabilities,
        })
    }
}

#[must_use]
pub fn render_rust_review(program: &Program, report: &AnalysisReport) -> ReviewDocument {
    let context = RenderContext::new(program);
    let mut text = String::from(
        "// Generated semantic review — READ ONLY.\n// This is not Rust source and cannot be executed.\n",
    );
    text.push_str(&format!(
        "// capability closure: [{}]\n\n",
        report.capability_closure.join(", ")
    ));
    for data_type in &program.data_types {
        text.push_str(&format!("enum {} {{\n", rust_type_name(&data_type.name)));
        for variant in &data_type.variants {
            text.push_str(&format!(
                "    {} {{ ",
                pascal_case(last_segment(&variant.name))
            ));
            for (index, field) in variant.fields.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                let field_type = field.type_expression.as_ref().map_or_else(
                    || "_".to_owned(),
                    |value| render_type(&Type::from_expression(value)),
                );
                text.push_str(&format!("{}: {field_type}", rust_value_name(&field.name)));
            }
            text.push_str(" },\n");
        }
        text.push_str("}\n\n");
    }

    let mut nodes = Vec::new();
    for definition in &program.definitions {
        let Some(analysis) = report
            .definitions
            .iter()
            .find(|analysis| analysis.name == definition.name)
        else {
            continue;
        };
        let source = module_source(program, &definition.name);
        let id = format!("definition:{}", definition.name);
        text.push_str(&format!(
            "// node: {id} | source: {source}:{}:{} | effects: [{}]\n",
            definition.expression.span.start.line,
            definition.expression.span.start.column,
            analysis.capabilities.join(", ")
        ));
        text.push_str(&render_definition(
            &definition.name,
            &definition.expression,
            &analysis.inferred_type,
            &context,
        ));
        text.push_str("\n\n");
        nodes.push(ReviewNode {
            id,
            source,
            start_line: definition.expression.span.start.line,
            start_column: definition.expression.span.start.column,
            end_line: definition.expression.span.end.line,
            end_column: definition.expression.span.end.column,
            inferred_type: analysis.inferred_type.clone(),
            capabilities: analysis.capabilities.clone(),
        });
    }
    ReviewDocument {
        renderer: "rust-readonly-v1",
        editable: false,
        text,
        nodes,
    }
}

fn render_definition(
    name: &str,
    expression: &Expression,
    inferred_type: &Type,
    context: &RenderContext,
) -> String {
    if let ExpressionKind::Function { parameters, body } = &expression.kind {
        let (parameter_types, result_type) = match inferred_type {
            Type::Function { parameters, result } => (parameters.as_slice(), result.as_ref()),
            _ => (&[][..], inferred_type),
        };
        let parameters = parameters
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let parameter_type = parameter_types
                    .get(index)
                    .map_or_else(|| "_".to_owned(), render_type);
                format!("{}: {parameter_type}", rust_value_name(name))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "fn {}({parameters}) -> {} {{\n    {}\n}}",
            rust_value_name(name),
            render_type(result_type),
            render_expression(body, context)
        )
    } else {
        format!(
            "let {}: {} = {};",
            rust_value_name(name),
            render_type(inferred_type),
            render_expression(expression, context)
        )
    }
}

fn render_expression(expression: &Expression, context: &RenderContext) -> String {
    match &expression.kind {
        ExpressionKind::Literal(datum) | ExpressionKind::Quote(datum) => datum.display(),
        ExpressionKind::Variable(name) => context.render_variable(name),
        ExpressionKind::If {
            condition,
            consequent,
            alternative,
        } => format!(
            "if {} {{ {} }} else {{ {} }}",
            render_expression(condition, context),
            render_expression(consequent, context),
            render_expression(alternative, context)
        ),
        ExpressionKind::And(items) => items
            .iter()
            .map(|item| render_expression(item, context))
            .collect::<Vec<_>>()
            .join(" && "),
        ExpressionKind::Or(items) => items
            .iter()
            .map(|item| render_expression(item, context))
            .collect::<Vec<_>>()
            .join(" || "),
        ExpressionKind::Cond {
            clauses,
            alternative,
        } => {
            let mut rendered = String::new();
            for (index, clause) in clauses.iter().enumerate() {
                rendered.push_str(if index == 0 { "if " } else { " else if " });
                rendered.push_str(&render_expression(&clause.condition, context));
                rendered.push_str(" { ");
                rendered.push_str(&render_expression(&clause.expression, context));
                rendered.push_str(" }");
            }
            rendered.push_str(" else { ");
            rendered.push_str(&render_expression(alternative, context));
            rendered.push_str(" }");
            rendered
        }
        ExpressionKind::Match { value, arms } => format!(
            "match {} {{ {} }}",
            render_expression(value, context),
            arms.iter()
                .map(|arm| format!(
                    "{} => {},",
                    render_pattern(&arm.pattern, context),
                    render_expression(&arm.expression, context)
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ExpressionKind::Let { bindings, body } => {
            let mut rendered = String::from("{ ");
            for binding in bindings {
                rendered.push_str(&format!(
                    "let {} = {}; ",
                    rust_value_name(&binding.name),
                    render_expression(&binding.expression, context)
                ));
            }
            rendered.push_str(&render_expression(body, context));
            rendered.push_str(" }");
            rendered
        }
        ExpressionKind::Function { parameters, body } => format!(
            "|{}| {}",
            parameters
                .iter()
                .map(|name| rust_value_name(name))
                .collect::<Vec<_>>()
                .join(", "),
            render_expression(body, context)
        ),
        ExpressionKind::Do(items) => format!(
            "{{ {} }}",
            items
                .iter()
                .map(|item| render_expression(item, context))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        ExpressionKind::Call { callee, arguments } => {
            if let ExpressionKind::Variable(name) = &callee.kind
                && let Some((data_type, variant, fields)) = context.constructors.get(name)
            {
                return format!(
                    "{}::{} {{ {} }}",
                    rust_type_name(data_type),
                    pascal_case(last_segment(variant)),
                    fields
                        .iter()
                        .zip(arguments)
                        .map(|(field, argument)| format!(
                            "{}: {}",
                            rust_value_name(field),
                            render_expression(argument, context)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if let ExpressionKind::Variable(operator) = &callee.kind
                && matches!(
                    operator.as_str(),
                    "+" | "-" | "*" | "=" | "<" | "<=" | ">" | ">="
                )
                && arguments.len() == 2
            {
                let operator = if operator == "=" { "==" } else { operator };
                return format!(
                    "({} {operator} {})",
                    render_expression(&arguments[0], context),
                    render_expression(&arguments[1], context)
                );
            }
            if callee.kind == ExpressionKind::Variable("map".to_owned())
                && arguments.len().is_multiple_of(2)
            {
                return format!(
                    "map! {{ {} }}",
                    arguments
                        .chunks_exact(2)
                        .map(|pair| format!(
                            "{} => {}",
                            render_expression(&pair[0], context),
                            render_expression(&pair[1], context)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            format!(
                "{}({})",
                render_expression(callee, context),
                arguments
                    .iter()
                    .map(|argument| render_expression(argument, context))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn render_pattern(pattern: &Pattern, context: &RenderContext) -> String {
    match &pattern.kind {
        PatternKind::Wildcard => "_".to_owned(),
        PatternKind::Binding(name) => rust_value_name(name),
        PatternKind::Literal(datum) => datum.display(),
        PatternKind::Variant { name, fields } => context.constructors.get(name).map_or_else(
            || {
                format!(
                    "{}({})",
                    context.render_variable(name),
                    fields
                        .iter()
                        .map(|field| render_pattern(field, context))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            |(data_type, variant, field_names)| {
                format!(
                    "{}::{} {{ {} }}",
                    rust_type_name(data_type),
                    pascal_case(last_segment(variant)),
                    field_names
                        .iter()
                        .zip(fields)
                        .map(|(field_name, pattern)| format!(
                            "{}: {}",
                            rust_value_name(field_name),
                            render_pattern(pattern, context)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        ),
    }
}

struct RenderContext {
    constructors: BTreeMap<String, (String, String, Vec<String>)>,
}

impl RenderContext {
    fn new(program: &Program) -> Self {
        let constructors = program
            .data_types
            .iter()
            .flat_map(|data_type| {
                data_type.variants.iter().map(|variant| {
                    (
                        variant.name.clone(),
                        (
                            data_type.name.clone(),
                            variant.name.clone(),
                            variant
                                .fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect(),
                        ),
                    )
                })
            })
            .collect();
        Self { constructors }
    }

    fn render_variable(&self, name: &str) -> String {
        self.constructors.get(name).map_or_else(
            || rust_value_name(name),
            |(data_type, variant, _)| {
                format!(
                    "{}::{}",
                    rust_type_name(data_type),
                    pascal_case(last_segment(variant))
                )
            },
        )
    }
}

fn render_type(value: &Type) -> String {
    match value {
        Type::List(item) => format!("Vec<{}>", render_type(item)),
        Type::Result { success, error } => {
            format!("Result<{}, {}>", render_type(success), render_type(error))
        }
        Type::User(name) => rust_type_name(name),
        Type::Schema(item) => format!("Schema<{}>", render_type(item)),
        Type::Function { parameters, result } => format!(
            "fn({}) -> {}",
            parameters
                .iter()
                .map(render_type)
                .collect::<Vec<_>>()
                .join(", "),
            render_type(result)
        ),
        _ => value.display(),
    }
}

fn module_source(program: &Program, name: &str) -> String {
    name.split_once('/')
        .map_or_else(|| program.name.clone(), |(module, _)| module.to_owned())
}

fn rust_value_name(name: &str) -> String {
    name.replace('/', "__").replace('-', "_").replace('?', "_q")
}

fn rust_type_name(name: &str) -> String {
    name.split(['/', '-'])
        .filter(|part| !part.is_empty())
        .map(pascal_case)
        .collect()
}

fn pascal_case(name: &str) -> String {
    let mut characters = name.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + characters.as_str()
    })
}

fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}
