#![forbid(unsafe_code)]

use std::collections::HashSet;

use ail_diagnostic::{AilResult, Diagnostic};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::json;

use crate::{
    Binding, Datum, DatumKind, Definition, Expression, ExpressionKind, LibraryRequirement, Program,
    Route, Schema, SchemaField, SchemaKind,
};

const SUPPORTED_CAPABILITIES: &[&str] = &["log", "kv", "clock"];
const SUPPORTED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
const EXPRESSION_KEYWORDS: &[&str] = &["quote", "if", "let", "fn", "do"];
const RESERVED_SCHEMA_NAMES: &[&str] = &[
    "+",
    "-",
    "*",
    "quotient",
    "remainder",
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
    "list",
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
    "api-response",
    "api-error",
    "log",
    "now-ms",
    "kv-get",
    "kv-put",
    "kv-delete",
    "kv-list",
];
const MAXIMUM_SCHEMAS: usize = 64;
const MAXIMUM_SCHEMA_DEPTH: usize = 16;
const MAXIMUM_OBJECT_FIELDS: usize = 64;
const MAXIMUM_SCHEMA_COLLECTION_LENGTH: u64 = 10_000;
const MAXIMUM_LIBRARY_COUNT: usize = 32;

pub fn parse_program(datum: &Datum, source: &str) -> AilResult<Program> {
    let top = datum.list().ok_or_else(|| {
        Diagnostic::simple("PROGRAM_EXPECTED", "top-level form must be (program ...)")
            .at(datum.span)
    })?;
    if top.first().and_then(Datum::symbol) != Some("program") {
        return Err(
            Diagnostic::simple("PROGRAM_EXPECTED", "top-level form must be (program ...)")
                .at(datum.span),
        );
    }

    let mut name = None;
    let mut version = None;
    let mut capabilities = None;
    let mut libraries = None;
    let mut exports = None;
    let mut schemas = Vec::new();
    let mut routes: Vec<Route> = Vec::new();
    let mut definitions = Vec::new();
    let mut schema_names = HashSet::new();
    let mut definition_names = HashSet::new();

    for form_datum in &top[1..] {
        let form = form_datum
            .list()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| {
                Diagnostic::new(
                    "PROGRAM_INVALID_FORM",
                    "program members must be non-empty forms",
                    json!({ "form": form_datum.display() }),
                )
                .at(form_datum.span)
            })?;
        let head = form[0].symbol().ok_or_else(|| {
            Diagnostic::new(
                "PROGRAM_INVALID_FORM",
                "program members must be non-empty forms",
                json!({ "form": form_datum.display() }),
            )
            .at(form_datum.span)
        })?;

        match head {
            "name" => {
                if form.len() != 2 || form[1].symbol().is_none() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_NAME",
                        "name must contain one symbol",
                    ));
                }
                if name.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_NAME",
                        "program has multiple name forms",
                    ));
                }
                name = form[1].symbol().map(str::to_owned);
            }
            "version" => {
                let Some(candidate) = exact_integer(form.get(1)) else {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_VERSION",
                        "version must contain one positive integer",
                    ));
                };
                if form.len() != 2 || candidate <= &BigInt::zero() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_VERSION",
                        "version must contain one positive integer",
                    ));
                }
                if version.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_VERSION",
                        "program has multiple version forms",
                    ));
                }
                version = Some(candidate.clone());
            }
            "capabilities" => {
                if capabilities.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_CAPABILITIES",
                        "program has multiple capabilities forms",
                    ));
                }
                let values = symbols(&form[1..]).ok_or_else(|| {
                    at(
                        form_datum,
                        "PROGRAM_INVALID_CAPABILITY",
                        "capability names must be symbols",
                    )
                })?;
                ensure_unique(
                    &values,
                    "PROGRAM_DUPLICATE_CAPABILITY",
                    "capability is declared more than once",
                    form_datum,
                )?;
                for capability in &values {
                    if !SUPPORTED_CAPABILITIES.contains(&capability.as_str()) {
                        return Err(Diagnostic::new(
                            "PROGRAM_UNKNOWN_CAPABILITY",
                            "program declares an unsupported capability",
                            json!({ "capability": capability }),
                        )
                        .at(form_datum.span));
                    }
                }
                capabilities = Some(values);
            }
            "libraries" => {
                if libraries.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_LIBRARIES",
                        "program has multiple libraries forms",
                    ));
                }
                if form.len() - 1 > MAXIMUM_LIBRARY_COUNT {
                    return Err(Diagnostic::new(
                        "PROGRAM_TOO_MANY_LIBRARIES",
                        "program declares too many libraries",
                        json!({ "maximum": MAXIMUM_LIBRARY_COUNT }),
                    )
                    .at(form_datum.span));
                }
                let mut requirements = Vec::new();
                let mut seen = HashSet::new();
                for declaration in &form[1..] {
                    let parts = declaration.list().unwrap_or_default();
                    let library_name = parts.first().and_then(Datum::symbol);
                    let library_version = exact_integer(parts.get(1)).and_then(ToPrimitive::to_u16);
                    if parts.len() != 2
                        || library_name.is_none_or(|value| !valid_library_name(value))
                        || library_version.is_none_or(|value| value == 0)
                    {
                        return Err(Diagnostic::new(
                            "PROGRAM_INVALID_LIBRARY",
                            "library declaration must be (lowercase-name VERSION)",
                            json!({ "library": declaration.display() }),
                        )
                        .at(declaration.span));
                    }
                    let library_name = library_name.unwrap_or_default().to_owned();
                    let library_version = library_version.unwrap_or_default();
                    if !seen.insert(library_name.clone()) {
                        return Err(Diagnostic::new(
                            "PROGRAM_DUPLICATE_LIBRARY",
                            "program declares a library more than once",
                            json!({ "library": library_name }),
                        )
                        .at(declaration.span));
                    }
                    if library_name != "text" || library_version != 1 {
                        return Err(Diagnostic::new(
                            "PROGRAM_UNKNOWN_LIBRARY",
                            "program declares an unsupported library contract",
                            json!({ "library": library_name, "version": library_version }),
                        )
                        .at(declaration.span));
                    }
                    requirements.push(LibraryRequirement {
                        name: library_name,
                        version: library_version,
                    });
                }
                libraries = Some(requirements);
            }
            "schema" => {
                if form.len() != 3 || form[1].symbol().is_none() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_SCHEMA",
                        "schema must be (schema name specification)",
                    ));
                }
                if schemas.len() >= MAXIMUM_SCHEMAS {
                    return Err(Diagnostic::new(
                        "PROGRAM_TOO_MANY_SCHEMAS",
                        "program declares too many schemas",
                        json!({ "maximum": MAXIMUM_SCHEMAS }),
                    )
                    .at(form_datum.span));
                }
                let schema_name = form[1].symbol().unwrap_or_default().to_owned();
                if RESERVED_SCHEMA_NAMES.contains(&schema_name.as_str()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_SCHEMA_RESERVED_NAME",
                        "schema name conflicts with a language or capability binding",
                        json!({ "name": schema_name }),
                    )
                    .at(form_datum.span));
                }
                if !schema_names.insert(schema_name.clone()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_SCHEMA",
                        "schema name is not unique",
                        json!({ "name": schema_name }),
                    )
                    .at(form_datum.span));
                }
                if definition_names.contains(&schema_name) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_BINDING",
                        "schema and definition names must be unique",
                        json!({ "name": schema_name }),
                    )
                    .at(form_datum.span));
                }
                schemas.push(Schema {
                    name: schema_name,
                    kind: parse_schema_specification(&form[2], 0)?,
                });
            }
            "route" => {
                if form.len() != 4
                    || form[1].symbol().is_none()
                    || !matches!(form[2].kind, DatumKind::String(_))
                    || form[3].symbol().is_none()
                {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_ROUTE",
                        "route must be (route METHOD \"/path\" handler)",
                    ));
                }
                let method = form[1].symbol().unwrap_or_default().to_uppercase();
                let DatumKind::String(path) = &form[2].kind else {
                    unreachable!()
                };
                let handler = form[3].symbol().unwrap_or_default().to_owned();
                if !SUPPORTED_METHODS.contains(&method.as_str()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_UNSUPPORTED_METHOD",
                        "route uses an unsupported HTTP method",
                        json!({ "method": method }),
                    )
                    .at(form_datum.span));
                }
                validate_route_path(path, &form[2])?;
                for existing in &routes {
                    if existing.method == method && route_patterns_overlap(path, &existing.path) {
                        return Err(Diagnostic::new(
                            "PROGRAM_AMBIGUOUS_ROUTE",
                            "route overlaps an earlier route for the same method",
                            json!({
                                "method": method,
                                "path": path,
                                "existingPath": existing.path,
                            }),
                        )
                        .at(form_datum.span));
                    }
                }
                routes.push(Route {
                    method,
                    path: path.clone(),
                    handler,
                });
            }
            "def" => {
                if form.len() != 3 || form[1].symbol().is_none() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_INVALID_DEFINITION",
                        "definition must be (def name expression)",
                    ));
                }
                let definition_name = form[1].symbol().unwrap_or_default().to_owned();
                if !definition_names.insert(definition_name.clone()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_DEFINITION",
                        "definition name is not unique",
                        json!({ "name": definition_name }),
                    )
                    .at(form_datum.span));
                }
                if schema_names.contains(&definition_name) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_BINDING",
                        "schema and definition names must be unique",
                        json!({ "name": definition_name }),
                    )
                    .at(form_datum.span));
                }
                definitions.push(Definition {
                    name: definition_name,
                    expression: parse_expression(&form[2])?,
                });
            }
            "export" => {
                if exports.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_EXPORT",
                        "program has multiple export forms",
                    ));
                }
                let values = symbols(&form[1..])
                    .filter(|values| !values.is_empty())
                    .ok_or_else(|| {
                        at(
                            form_datum,
                            "PROGRAM_INVALID_EXPORT",
                            "export must contain at least one symbol",
                        )
                    })?;
                ensure_unique(
                    &values,
                    "PROGRAM_DUPLICATE_EXPORT_NAME",
                    "export name is listed more than once",
                    form_datum,
                )?;
                exports = Some(values);
            }
            _ => {
                return Err(Diagnostic::new(
                    "PROGRAM_UNKNOWN_FORM",
                    "unknown top-level program form",
                    json!({ "form": head }),
                )
                .at(form_datum.span));
            }
        }
    }

    let name = name.ok_or_else(|| {
        Diagnostic::simple("PROGRAM_MISSING_NAME", "program is missing a name form").at(datum.span)
    })?;
    let version = version.ok_or_else(|| {
        Diagnostic::simple(
            "PROGRAM_MISSING_VERSION",
            "program is missing a version form",
        )
        .at(datum.span)
    })?;
    let exports = exports.ok_or_else(|| {
        Diagnostic::simple(
            "PROGRAM_MISSING_EXPORT",
            "program is missing an export form",
        )
        .at(datum.span)
    })?;
    for export_name in &exports {
        if !definition_names.contains(export_name) {
            return Err(Diagnostic::new(
                "PROGRAM_UNKNOWN_EXPORT",
                "export does not name a program definition",
                json!({ "name": export_name }),
            )
            .at(datum.span));
        }
    }
    for route in &routes {
        if !definition_names.contains(&route.handler) {
            return Err(Diagnostic::new(
                "PROGRAM_UNKNOWN_ROUTE_HANDLER",
                "route handler does not name a program definition",
                json!({ "handler": route.handler }),
            )
            .at(datum.span));
        }
        if !exports.contains(&route.handler) {
            return Err(Diagnostic::new(
                "PROGRAM_ROUTE_HANDLER_NOT_EXPORTED",
                "route handler must be exported",
                json!({ "handler": route.handler }),
            )
            .at(datum.span));
        }
    }
    let libraries = libraries.unwrap_or_default();
    for requirement in &libraries {
        let namespace = format!("{}/", requirement.name);
        for binding_name in schema_names.iter().chain(definition_names.iter()) {
            if binding_name.starts_with(&namespace) {
                return Err(Diagnostic::new(
                    "PROGRAM_LIBRARY_NAMESPACE_CONFLICT",
                    "guest binding occupies a declared library namespace",
                    json!({ "library": requirement.name, "binding": binding_name }),
                )
                .at(datum.span));
            }
        }
    }

    Ok(Program {
        name,
        version,
        capabilities: capabilities.unwrap_or_default(),
        libraries,
        schemas,
        routes,
        definitions,
        exports,
        source: source.to_owned(),
    })
}

fn parse_schema_specification(datum: &Datum, depth: usize) -> AilResult<SchemaKind> {
    if depth > MAXIMUM_SCHEMA_DEPTH {
        return Err(Diagnostic::new(
            "PROGRAM_SCHEMA_TOO_DEEP",
            "schema exceeds the maximum nesting depth",
            json!({ "maximum": MAXIMUM_SCHEMA_DEPTH }),
        )
        .at(datum.span));
    }
    match datum.symbol() {
        Some("any") => return Ok(SchemaKind::Any),
        Some("string") => {
            return Ok(SchemaKind::String {
                minimum_length: BigInt::zero(),
                maximum_length: None,
            });
        }
        Some("integer") => {
            return Ok(SchemaKind::Integer {
                minimum: None,
                maximum: None,
            });
        }
        Some("boolean") => return Ok(SchemaKind::Boolean),
        _ => {}
    }

    let Some(form) = datum.list().filter(|form| !form.is_empty()) else {
        return Err(invalid_schema(datum, "unknown schema specification"));
    };
    match form[0].symbol() {
        Some("string") => {
            let minimum = exact_integer(form.get(1));
            let maximum = exact_integer(form.get(2));
            if form.len() != 3
                || minimum.is_none_or(|value| value < &BigInt::zero())
                || maximum.is_none_or(|value| value < &BigInt::zero())
                || minimum > maximum
            {
                return Err(invalid_schema(
                    datum,
                    "bounded string must be (string MINIMUM MAXIMUM)",
                ));
            }
            Ok(SchemaKind::String {
                minimum_length: minimum.cloned().unwrap_or_else(BigInt::zero),
                maximum_length: maximum.cloned(),
            })
        }
        Some("integer") => {
            let minimum = exact_integer(form.get(1));
            let maximum = exact_integer(form.get(2));
            if form.len() != 3 || minimum.is_none() || maximum.is_none() || minimum > maximum {
                return Err(invalid_schema(
                    datum,
                    "bounded integer must be (integer MINIMUM MAXIMUM)",
                ));
            }
            Ok(SchemaKind::Integer {
                minimum: minimum.cloned(),
                maximum: maximum.cloned(),
            })
        }
        Some("list") => {
            let minimum = exact_integer(form.get(2)).and_then(ToPrimitive::to_u64);
            let maximum = exact_integer(form.get(3)).and_then(ToPrimitive::to_u64);
            if form.len() != 4
                || minimum.is_none()
                || maximum.is_none()
                || minimum > maximum
                || maximum.is_some_and(|value| value > MAXIMUM_SCHEMA_COLLECTION_LENGTH)
            {
                return Err(invalid_schema(
                    datum,
                    "list must be (list ITEM MINIMUM MAXIMUM) with bounded lengths",
                ));
            }
            Ok(SchemaKind::List {
                item: Box::new(parse_schema_specification(&form[1], depth + 1)?),
                minimum_length: minimum.unwrap_or_default(),
                maximum_length: maximum.unwrap_or_default(),
            })
        }
        Some("object") => {
            if form.len() - 1 > MAXIMUM_OBJECT_FIELDS {
                return Err(Diagnostic::new(
                    "PROGRAM_SCHEMA_TOO_MANY_FIELDS",
                    "object schema declares too many fields",
                    json!({ "maximum": MAXIMUM_OBJECT_FIELDS }),
                )
                .at(datum.span));
            }
            let mut fields = Vec::new();
            let mut names = HashSet::new();
            for raw_field in &form[1..] {
                let field = parse_schema_field(raw_field, depth + 1)?;
                if !names.insert(field.name.clone()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_SCHEMA_DUPLICATE_FIELD",
                        "object schema field name is not unique",
                        json!({ "field": field.name }),
                    )
                    .at(raw_field.span));
                }
                fields.push(field);
            }
            Ok(SchemaKind::Object { fields })
        }
        _ => Err(invalid_schema(datum, "unknown schema constructor")),
    }
}

fn parse_schema_field(datum: &Datum, depth: usize) -> AilResult<SchemaField> {
    let Some(form) = datum.list().filter(|form| form.len() >= 3) else {
        return Err(invalid_schema_field(
            datum,
            "schema field must have a bounded string name",
        ));
    };
    let constructor = form[0].symbol();
    let DatumKind::String(name) = &form[1].kind else {
        return Err(invalid_schema_field(
            datum,
            "schema field must have a bounded string name",
        ));
    };
    if !matches!(constructor, Some("required" | "optional"))
        || name.is_empty()
        || name.chars().count() > 128
    {
        return Err(invalid_schema_field(
            datum,
            "schema field must have a bounded string name",
        ));
    }
    let required = constructor == Some("required");
    if (required && form.len() != 3) || (!required && !matches!(form.len(), 3 | 4)) {
        return Err(invalid_schema_field(
            datum,
            if required {
                "required field must be (required \"name\" SCHEMA)"
            } else {
                "optional field must be (optional \"name\" SCHEMA [DEFAULT])"
            },
        ));
    }
    let specification = parse_schema_specification(&form[2], depth)?;
    let default = form.get(3).cloned();
    if let Some(default_value) = &default
        && (!portable_default(default_value) || !schema_accepts(&specification, default_value))
    {
        return Err(Diagnostic::new(
            "PROGRAM_SCHEMA_INVALID_DEFAULT",
            "schema default does not satisfy its field schema",
            json!({ "field": name }),
        )
        .at(default_value.span));
    }
    Ok(SchemaField {
        name: name.clone(),
        specification,
        required,
        default,
    })
}

fn parse_expression(datum: &Datum) -> AilResult<Expression> {
    let kind = match &datum.kind {
        DatumKind::Integer(_) | DatumKind::Bool(_) | DatumKind::String(_) => {
            ExpressionKind::Literal(datum.clone())
        }
        DatumKind::Symbol(name) => ExpressionKind::Variable(name.clone()),
        DatumKind::List(form) if form.is_empty() => ExpressionKind::Literal(datum.clone()),
        DatumKind::List(form) => {
            let head = &form[0];
            match head.symbol() {
                Some("quote") => {
                    if form.len() != 2 {
                        return Err(invalid_special_form("quote", datum));
                    }
                    ExpressionKind::Quote(form[1].clone())
                }
                Some("if") => {
                    if form.len() != 4 {
                        return Err(invalid_special_form("if", datum));
                    }
                    ExpressionKind::If {
                        condition: Box::new(parse_expression(&form[1])?),
                        consequent: Box::new(parse_expression(&form[2])?),
                        alternative: Box::new(parse_expression(&form[3])?),
                    }
                }
                Some("let") => {
                    if form.len() != 3 {
                        return Err(invalid_special_form("let", datum));
                    }
                    let raw_bindings = form[1].list().ok_or_else(|| {
                        at(
                            &form[1],
                            "PARSE_INVALID_LET_BINDINGS",
                            "let bindings must be a proper list",
                        )
                    })?;
                    let mut names = Vec::new();
                    let mut bindings = Vec::new();
                    for raw_binding in raw_bindings {
                        let pair = raw_binding.list().unwrap_or_default();
                        if pair.len() != 2 || pair[0].symbol().is_none() {
                            return Err(Diagnostic::new(
                                "PARSE_INVALID_LET_BINDING",
                                "let binding must be (name expression)",
                                json!({ "binding": raw_binding.display() }),
                            )
                            .at(raw_binding.span));
                        }
                        let name = pair[0].symbol().unwrap_or_default().to_owned();
                        names.push(name.clone());
                        bindings.push(Binding {
                            name,
                            expression: parse_expression(&pair[1])?,
                        });
                    }
                    ensure_unique(
                        &names,
                        "PARSE_DUPLICATE_LET_BINDING",
                        "let binding name is not unique",
                        &form[1],
                    )?;
                    ExpressionKind::Let {
                        bindings,
                        body: Box::new(parse_expression(&form[2])?),
                    }
                }
                Some("fn") => {
                    if form.len() != 3 {
                        return Err(invalid_special_form("fn", datum));
                    }
                    let parameters = form[1].list().and_then(symbols).ok_or_else(|| {
                        at(
                            &form[1],
                            "PARSE_INVALID_PARAMETERS",
                            "function parameters must be a proper list of symbols",
                        )
                    })?;
                    ensure_unique(
                        &parameters,
                        "PARSE_DUPLICATE_PARAMETER",
                        "function parameter name is not unique",
                        &form[1],
                    )?;
                    ExpressionKind::Function {
                        parameters,
                        body: Box::new(parse_expression(&form[2])?),
                    }
                }
                Some("do") => {
                    if form.len() == 1 {
                        return Err(invalid_special_form("do", datum));
                    }
                    ExpressionKind::Do(
                        form[1..]
                            .iter()
                            .map(parse_expression)
                            .collect::<AilResult<Vec<_>>>()?,
                    )
                }
                Some(keyword) if EXPRESSION_KEYWORDS.contains(&keyword) => {
                    return Err(invalid_special_form(keyword, datum));
                }
                _ => ExpressionKind::Call {
                    callee: Box::new(parse_expression(head)?),
                    arguments: form[1..]
                        .iter()
                        .map(parse_expression)
                        .collect::<AilResult<Vec<_>>>()?,
                },
            }
        }
    };
    Ok(Expression {
        kind,
        span: datum.span,
    })
}

fn exact_integer(datum: Option<&Datum>) -> Option<&BigInt> {
    match datum.map(|value| &value.kind) {
        Some(DatumKind::Integer(value)) => Some(value),
        _ => None,
    }
}

fn symbols(values: &[Datum]) -> Option<Vec<String>> {
    values
        .iter()
        .map(|value| value.symbol().map(str::to_owned))
        .collect()
}

fn ensure_unique(
    values: &[String],
    code: &'static str,
    message: &'static str,
    datum: &Datum,
) -> AilResult<()> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(Diagnostic::new(code, message, json!({ "name": value })).at(datum.span));
        }
    }
    Ok(())
}

fn valid_library_name(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && value.len() <= 64
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn portable_default(datum: &Datum) -> bool {
    match &datum.kind {
        DatumKind::Integer(_) | DatumKind::Bool(_) | DatumKind::String(_) => true,
        DatumKind::List(values) => values.iter().all(portable_default),
        DatumKind::Symbol(_) => false,
    }
}

fn schema_accepts(schema: &SchemaKind, datum: &Datum) -> bool {
    match (schema, &datum.kind) {
        (SchemaKind::Any, _) => true,
        (
            SchemaKind::String {
                minimum_length,
                maximum_length,
            },
            DatumKind::String(value),
        ) => {
            let length = BigInt::from(value.chars().count());
            &length >= minimum_length
                && maximum_length
                    .as_ref()
                    .is_none_or(|maximum| &length <= maximum)
        }
        (SchemaKind::Integer { minimum, maximum }, DatumKind::Integer(value)) => {
            minimum.as_ref().is_none_or(|minimum| value >= minimum)
                && maximum.as_ref().is_none_or(|maximum| value <= maximum)
        }
        (SchemaKind::Boolean, DatumKind::Bool(_)) => true,
        (
            SchemaKind::List {
                item,
                minimum_length,
                maximum_length,
            },
            DatumKind::List(values),
        ) => {
            let length = u64::try_from(values.len()).unwrap_or(u64::MAX);
            length >= *minimum_length
                && length <= *maximum_length
                && values.iter().all(|value| schema_accepts(item, value))
        }
        (SchemaKind::Object { .. }, _) => false,
        _ => false,
    }
}

fn invalid_schema(datum: &Datum, message: &'static str) -> Diagnostic {
    Diagnostic::new(
        "PROGRAM_INVALID_SCHEMA_SPECIFICATION",
        message,
        json!({ "schema": datum.display() }),
    )
    .at(datum.span)
}

fn invalid_schema_field(datum: &Datum, message: &'static str) -> Diagnostic {
    Diagnostic::new(
        "PROGRAM_INVALID_SCHEMA_FIELD",
        message,
        json!({ "field": datum.display() }),
    )
    .at(datum.span)
}

fn invalid_special_form(name: &str, datum: &Datum) -> Diagnostic {
    Diagnostic::new(
        "PARSE_INVALID_SPECIAL_FORM",
        "special form has an invalid shape",
        json!({ "form": name, "datum": datum.display() }),
    )
    .at(datum.span)
}

fn validate_route_path(path: &str, datum: &Datum) -> AilResult<()> {
    if path.is_empty()
        || path.chars().count() > 2048
        || !path.starts_with('/')
        || path.contains('?')
        || path.contains('#')
        || path.chars().any(char::is_whitespace)
    {
        return Err(Diagnostic::new(
            "PROGRAM_INVALID_ROUTE_PATH",
            "route path must be an absolute path without query or fragment",
            json!({ "path": path }),
        )
        .at(datum.span));
    }
    let segments = route_segments(path);
    if segments.contains(&"") {
        return Err(Diagnostic::new(
            "PROGRAM_INVALID_ROUTE_PATH",
            "route path cannot contain empty segments or a trailing slash",
            json!({ "path": path }),
        )
        .at(datum.span));
    }
    let mut parameters = HashSet::new();
    for segment in segments {
        if let Some(parameter) = segment.strip_prefix(':') {
            if !valid_route_parameter(parameter) {
                return Err(Diagnostic::new(
                    "PROGRAM_INVALID_ROUTE_PARAMETER",
                    "route parameter has an invalid name",
                    json!({ "path": path, "segment": segment }),
                )
                .at(datum.span));
            }
            if !parameters.insert(parameter) {
                return Err(Diagnostic::new(
                    "PROGRAM_DUPLICATE_ROUTE_PARAMETER",
                    "route parameter name is repeated",
                    json!({ "path": path, "parameter": parameter }),
                )
                .at(datum.span));
            }
        }
    }
    Ok(())
}

fn valid_route_parameter(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn route_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        Vec::new()
    } else {
        path.get(1..)
            .map_or_else(Vec::new, |value| value.split('/').collect())
    }
}

fn route_patterns_overlap(left: &str, right: &str) -> bool {
    let left_segments = route_segments(left);
    let right_segments = route_segments(right);
    left_segments.len() == right_segments.len()
        && left_segments
            .iter()
            .zip(right_segments)
            .all(|(left, right)| left.starts_with(':') || right.starts_with(':') || left == &right)
}

fn at(datum: &Datum, code: &'static str, message: &'static str) -> Diagnostic {
    Diagnostic::simple(code, message).at(datum.span)
}

#[cfg(test)]
mod tests {
    use ail_diagnostic::{AilResult, Diagnostic};
    use serde_json::json;

    use crate::load_program_source;

    const CORE: &str = include_str!("../../../../conformance/v1/programs/core.ail");
    const SCHEMA: &str = include_str!("../../../../conformance/v1/programs/schema.ail");
    const LIBRARY: &str = include_str!("../../../../conformance/v1/programs/library.ail");
    const UNKNOWN_LIBRARY: &str =
        include_str!("../../../../conformance/v1/invalid/unknown-library.ail");

    fn require_error<T>(result: AilResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    #[test]
    fn parses_conformance_program_summaries() {
        let cases = [
            (
                CORE,
                json!({
                    "name": "conformance-core",
                    "version": 1,
                    "capabilities": [],
                    "libraries": [],
                    "schemas": [],
                    "routes": [],
                    "definitions": ["factorial", "sequential-let", "truthy", "big-add", "divide-by-zero", "forever"],
                    "exports": ["factorial", "sequential-let", "truthy", "big-add", "divide-by-zero", "forever"]
                }),
            ),
            (
                SCHEMA,
                json!({
                    "name": "conformance-schema",
                    "version": 1,
                    "capabilities": [],
                    "libraries": [],
                    "schemas": ["input"],
                    "routes": [],
                    "definitions": ["check"],
                    "exports": ["check"]
                }),
            ),
            (
                LIBRARY,
                json!({
                    "name": "conformance-library",
                    "version": 1,
                    "capabilities": [],
                    "libraries": [{"name": "text", "version": 1}],
                    "schemas": [],
                    "routes": [],
                    "definitions": ["inspect", "measure"],
                    "exports": ["inspect", "measure"]
                }),
            ),
        ];
        for (source, expected) in cases {
            let program =
                load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
            assert_eq!(program.summary_json(), expected);
        }
    }

    #[test]
    fn reports_unknown_library_like_the_reference_parser() {
        let diagnostic = require_error(load_program_source(UNKNOWN_LIBRARY));
        assert_eq!(diagnostic.code, "PROGRAM_UNKNOWN_LIBRARY");
        assert_eq!(
            diagnostic.message.as_ref(),
            "program declares an unsupported library contract"
        );
        assert_eq!(
            diagnostic.details.as_ref(),
            &json!({ "library": "text", "version": 2 })
        );
    }

    #[test]
    fn retains_byte_and_line_spans() {
        let program = load_program_source(CORE).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let expression = &program.definitions[0].expression;
        assert!(expression.span.start.offset < expression.span.end.offset);
        assert!(expression.span.start.line >= 1);
    }
}
