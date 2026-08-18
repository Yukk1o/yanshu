#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::{
    Binding, CondClause, DataTypeDefinition, Definition, Expression, ExpressionKind,
    LibraryRequirement, MatchArm, Pattern, PatternKind, Program, Route, Schema, VariantDefinition,
};
use serde_json::json;

pub(crate) fn link_programs(
    programs: &BTreeMap<String, Program>,
    order: &[String],
    entry: &str,
) -> AilResult<Program> {
    let entry_program = programs.get(entry).ok_or_else(|| {
        Diagnostic::new(
            "BUNDLE_ENTRY_MISSING",
            "bundle entry module is missing",
            json!({ "entry": entry }),
        )
    })?;
    let mut exports = BTreeMap::new();
    let mut top_bindings = BTreeMap::new();
    for (module_name, program) in programs {
        let bindings = module_bindings(module_name, program);
        for export in &program.exports {
            let target = bindings.get(export).ok_or_else(|| {
                Diagnostic::new(
                    "BUNDLE_INVALID_EXPORT",
                    "module export has no linkable binding",
                    json!({ "module": module_name, "export": export }),
                )
            })?;
            exports.insert((module_name.clone(), export.clone()), target.clone());
        }
        top_bindings.insert(module_name.clone(), bindings);
    }

    let mut capabilities = BTreeSet::new();
    let mut libraries: BTreeMap<String, LibraryRequirement> = BTreeMap::new();
    let mut data_types = Vec::new();
    let mut schemas = Vec::new();
    let mut definitions = Vec::new();
    for module_name in order {
        let program = programs.get(module_name).ok_or_else(|| {
            Diagnostic::new(
                "BUNDLE_IMPORT_MISSING",
                "bundle dependency order names an unknown module",
                json!({ "module": module_name }),
            )
        })?;
        if module_name != entry && !program.routes.is_empty() {
            return Err(Diagnostic::new(
                "BUNDLE_ROUTE_OUTSIDE_ENTRY",
                "only the bundle entry module may declare routes",
                json!({ "module": module_name }),
            ));
        }
        capabilities.extend(program.capabilities.iter().cloned());
        for library in &program.libraries {
            if let Some(existing) = libraries.get(&library.name)
                && existing.version != library.version
            {
                return Err(Diagnostic::new(
                    "BUNDLE_LIBRARY_VERSION_CONFLICT",
                    "bundle modules require incompatible library contracts",
                    json!({ "library": library.name, "left": existing.version, "right": library.version }),
                ));
            }
            libraries.insert(library.name.clone(), library.clone());
        }

        let local = top_bindings.get(module_name).ok_or_else(|| {
            Diagnostic::simple("BUNDLE_LINK_INTERNAL", "module binding table is missing")
        })?;
        let imported = imported_bindings(program, local, &exports)?;
        for data_type in &program.data_types {
            data_types.push(DataTypeDefinition {
                name: qualify(module_name, &data_type.name),
                variants: data_type
                    .variants
                    .iter()
                    .map(|variant| VariantDefinition {
                        name: qualify(module_name, &variant.name),
                        fields: variant.fields.clone(),
                    })
                    .collect(),
            });
        }
        schemas.extend(program.schemas.iter().map(|schema| Schema {
            name: qualify(module_name, &schema.name),
            kind: schema.kind.clone(),
        }));
        for definition in &program.definitions {
            definitions.push(Definition {
                name: qualify(module_name, &definition.name),
                expression: resolve_expression(
                    &definition.expression,
                    local,
                    &imported,
                    &BTreeSet::new(),
                ),
            });
        }
    }

    for export in &entry_program.exports {
        let qualified = exports
            .get(&(entry.to_owned(), export.clone()))
            .ok_or_else(|| {
                Diagnostic::new(
                    "BUNDLE_INVALID_EXPORT",
                    "entry export has no linkable binding",
                    json!({ "export": export }),
                )
            })?;
        definitions.push(Definition {
            name: export.clone(),
            expression: Expression {
                kind: ExpressionKind::Variable(qualified.clone()),
                span: entry_program
                    .definitions
                    .first()
                    .map_or_else(default_span, |definition| definition.expression.span),
            },
        });
    }

    Ok(Program {
        name: entry.to_owned(),
        version: entry_program.version.clone(),
        imports: Vec::new(),
        capabilities: capabilities.into_iter().collect(),
        libraries: libraries.into_values().collect(),
        data_types,
        schemas,
        routes: entry_program
            .routes
            .iter()
            .map(|route| Route {
                method: route.method.clone(),
                path: route.path.clone(),
                handler: route.handler.clone(),
            })
            .collect(),
        definitions,
        exports: entry_program.exports.clone(),
        source: String::new(),
    })
}

fn module_bindings(module: &str, program: &Program) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    for schema in &program.schemas {
        bindings.insert(schema.name.clone(), qualify(module, &schema.name));
    }
    for data_type in &program.data_types {
        for variant in &data_type.variants {
            bindings.insert(variant.name.clone(), qualify(module, &variant.name));
        }
    }
    for definition in &program.definitions {
        bindings.insert(definition.name.clone(), qualify(module, &definition.name));
    }
    bindings
}

fn imported_bindings(
    program: &Program,
    local: &BTreeMap<String, String>,
    exports: &BTreeMap<(String, String), String>,
) -> AilResult<BTreeMap<String, String>> {
    let mut imported = BTreeMap::new();
    for module in &program.imports {
        for ((export_module, name), target) in exports {
            if export_module != module {
                continue;
            }
            if local.contains_key(name) || imported.contains_key(name) {
                return Err(Diagnostic::new(
                    "BUNDLE_AMBIGUOUS_IMPORT",
                    "imported binding conflicts with another visible binding",
                    json!({ "module": program.name, "binding": name }),
                ));
            }
            imported.insert(name.clone(), target.clone());
        }
    }
    Ok(imported)
}

fn resolve_expression(
    expression: &Expression,
    local: &BTreeMap<String, String>,
    imported: &BTreeMap<String, String>,
    lexical: &BTreeSet<String>,
) -> Expression {
    let resolve = |name: &str| {
        if lexical.contains(name) {
            name.to_owned()
        } else {
            local
                .get(name)
                .or_else(|| imported.get(name))
                .cloned()
                .unwrap_or_else(|| name.to_owned())
        }
    };
    let kind = match &expression.kind {
        ExpressionKind::Literal(value) => ExpressionKind::Literal(value.clone()),
        ExpressionKind::Quote(value) => ExpressionKind::Quote(value.clone()),
        ExpressionKind::Variable(name) => ExpressionKind::Variable(resolve(name)),
        ExpressionKind::If {
            condition,
            consequent,
            alternative,
        } => ExpressionKind::If {
            condition: Box::new(resolve_expression(condition, local, imported, lexical)),
            consequent: Box::new(resolve_expression(consequent, local, imported, lexical)),
            alternative: Box::new(resolve_expression(alternative, local, imported, lexical)),
        },
        ExpressionKind::And(items) => ExpressionKind::And(
            items
                .iter()
                .map(|item| resolve_expression(item, local, imported, lexical))
                .collect(),
        ),
        ExpressionKind::Or(items) => ExpressionKind::Or(
            items
                .iter()
                .map(|item| resolve_expression(item, local, imported, lexical))
                .collect(),
        ),
        ExpressionKind::Cond {
            clauses,
            alternative,
        } => ExpressionKind::Cond {
            clauses: clauses
                .iter()
                .map(|clause| CondClause {
                    condition: resolve_expression(&clause.condition, local, imported, lexical),
                    expression: resolve_expression(&clause.expression, local, imported, lexical),
                })
                .collect(),
            alternative: Box::new(resolve_expression(alternative, local, imported, lexical)),
        },
        ExpressionKind::Match { value, arms } => ExpressionKind::Match {
            value: Box::new(resolve_expression(value, local, imported, lexical)),
            arms: arms
                .iter()
                .map(|arm| {
                    let mut arm_lexical = lexical.clone();
                    collect_pattern_bindings(&arm.pattern, &mut arm_lexical);
                    MatchArm {
                        pattern: resolve_pattern(&arm.pattern, local, imported),
                        expression: resolve_expression(
                            &arm.expression,
                            local,
                            imported,
                            &arm_lexical,
                        ),
                    }
                })
                .collect(),
        },
        ExpressionKind::Let { bindings, body } => {
            let mut let_lexical = lexical.clone();
            let mut resolved = Vec::with_capacity(bindings.len());
            for binding in bindings {
                resolved.push(Binding {
                    name: binding.name.clone(),
                    expression: resolve_expression(
                        &binding.expression,
                        local,
                        imported,
                        &let_lexical,
                    ),
                });
                let_lexical.insert(binding.name.clone());
            }
            ExpressionKind::Let {
                bindings: resolved,
                body: Box::new(resolve_expression(body, local, imported, &let_lexical)),
            }
        }
        ExpressionKind::Function { parameters, body } => {
            let mut function_lexical = lexical.clone();
            function_lexical.extend(parameters.iter().cloned());
            ExpressionKind::Function {
                parameters: parameters.clone(),
                body: Box::new(resolve_expression(body, local, imported, &function_lexical)),
            }
        }
        ExpressionKind::Do(items) => ExpressionKind::Do(
            items
                .iter()
                .map(|item| resolve_expression(item, local, imported, lexical))
                .collect(),
        ),
        ExpressionKind::Call { callee, arguments } => ExpressionKind::Call {
            callee: Box::new(resolve_expression(callee, local, imported, lexical)),
            arguments: arguments
                .iter()
                .map(|argument| resolve_expression(argument, local, imported, lexical))
                .collect(),
        },
    };
    Expression {
        kind,
        span: expression.span,
    }
}

fn resolve_pattern(
    pattern: &Pattern,
    local: &BTreeMap<String, String>,
    imported: &BTreeMap<String, String>,
) -> Pattern {
    let kind = match &pattern.kind {
        PatternKind::Wildcard => PatternKind::Wildcard,
        PatternKind::Binding(name) => PatternKind::Binding(name.clone()),
        PatternKind::Literal(value) => PatternKind::Literal(value.clone()),
        PatternKind::Variant { name, fields } => PatternKind::Variant {
            name: local
                .get(name)
                .or_else(|| imported.get(name))
                .cloned()
                .unwrap_or_else(|| name.clone()),
            fields: fields
                .iter()
                .map(|field| resolve_pattern(field, local, imported))
                .collect(),
        },
    };
    Pattern {
        kind,
        span: pattern.span,
    }
}

fn collect_pattern_bindings(pattern: &Pattern, bindings: &mut BTreeSet<String>) {
    match &pattern.kind {
        PatternKind::Binding(name) => {
            bindings.insert(name.clone());
        }
        PatternKind::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(field, bindings);
            }
        }
        PatternKind::Wildcard | PatternKind::Literal(_) => {}
    }
}

fn qualify(module: &str, name: &str) -> String {
    format!("{module}/{name}")
}

fn default_span() -> ail_diagnostic::Span {
    let position = ail_diagnostic::Position {
        offset: 0,
        line: 1,
        column: 1,
    };
    ail_diagnostic::Span {
        start: position,
        end: position,
    }
}
