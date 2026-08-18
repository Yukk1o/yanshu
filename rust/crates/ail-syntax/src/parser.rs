#![forbid(unsafe_code)]

use std::collections::HashSet;

use ail_diagnostic::{AilResult, Diagnostic};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::json;

use crate::{
    Binding, CondClause, DataField, DataTypeDefinition, Datum, DatumKind, Definition, Expression,
    ExpressionKind, FunctionSignature, LibraryRequirement, MatchArm, Pattern, PatternKind, Program,
    Route, Schema, SchemaField, SchemaKind, TypeExpression, VariantDefinition,
};

const SUPPORTED_CAPABILITIES: &[&str] = &["log", "kv", "clock"];
const SUPPORTED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
const EXPRESSION_KEYWORDS: &[&str] = &[
    "quote", "if", "and", "or", "cond", "match", "let", "fn", "do",
];
const RESERVED_SCHEMA_NAMES: &[&str] = &[
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
const MAXIMUM_SCHEMAS: usize = 64;
const MAXIMUM_SCHEMA_DEPTH: usize = 16;
const MAXIMUM_OBJECT_FIELDS: usize = 64;
const MAXIMUM_SCHEMA_COLLECTION_LENGTH: u64 = 10_000;
const MAXIMUM_LIBRARY_COUNT: usize = 32;
const MAXIMUM_LANGUAGE_VERSION: u8 = 4;
const MAXIMUM_ENUM_VALUES: usize = 64;
const MAXIMUM_UNION_VARIANTS: usize = 8;
const MAXIMUM_IMPORTS: usize = 64;
const MAXIMUM_DATA_TYPES: usize = 64;
const MAXIMUM_DATA_VARIANTS: usize = 64;
const MAXIMUM_VARIANT_FIELDS: usize = 64;
const MAXIMUM_SIGNATURES: usize = 256;
const MAXIMUM_TYPE_DEPTH: usize = 16;

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
    let mut imports = None;
    let mut exports = None;
    let mut type_exports = None;
    let mut schemas = Vec::new();
    let mut data_types = Vec::new();
    let mut signatures = Vec::new();
    let mut routes: Vec<Route> = Vec::new();
    let mut definitions = Vec::new();
    let mut schema_names = HashSet::new();
    let mut data_type_names = HashSet::new();
    let mut constructor_names = HashSet::new();
    let mut signature_names = HashSet::new();
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
            "imports" => {
                if imports.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_IMPORTS",
                        "program has multiple imports forms",
                    ));
                }
                if form.len().saturating_sub(1) > MAXIMUM_IMPORTS {
                    return Err(Diagnostic::new(
                        "PROGRAM_TOO_MANY_IMPORTS",
                        "program declares too many module imports",
                        json!({ "maximum": MAXIMUM_IMPORTS }),
                    )
                    .at(form_datum.span));
                }
                let values = symbols(&form[1..]).ok_or_else(|| {
                    at(
                        form_datum,
                        "PROGRAM_INVALID_IMPORT",
                        "module import names must be symbols",
                    )
                })?;
                ensure_unique(
                    &values,
                    "PROGRAM_DUPLICATE_IMPORT",
                    "module is imported more than once",
                    form_datum,
                )?;
                imports = Some(values);
            }
            "data" => {
                let definition = parse_data_type(form, form_datum)?;
                if data_types.len() >= MAXIMUM_DATA_TYPES {
                    return Err(Diagnostic::new(
                        "PROGRAM_TOO_MANY_DATA_TYPES",
                        "program declares too many data types",
                        json!({ "maximum": MAXIMUM_DATA_TYPES }),
                    )
                    .at(form_datum.span));
                }
                if !data_type_names.insert(definition.name.clone()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_DATA_TYPE",
                        "data type name is not unique",
                        json!({ "name": definition.name }),
                    )
                    .at(form_datum.span));
                }
                for variant in &definition.variants {
                    if RESERVED_SCHEMA_NAMES.contains(&variant.name.as_str())
                        || EXPRESSION_KEYWORDS.contains(&variant.name.as_str())
                    {
                        return Err(Diagnostic::new(
                            "PROGRAM_CONSTRUCTOR_RESERVED_NAME",
                            "constructor name conflicts with a language or capability binding",
                            json!({ "name": variant.name }),
                        )
                        .at(form_datum.span));
                    }
                    if !constructor_names.insert(variant.name.clone()) {
                        return Err(Diagnostic::new(
                            "PROGRAM_DUPLICATE_CONSTRUCTOR",
                            "constructor name is not unique",
                            json!({ "name": variant.name }),
                        )
                        .at(form_datum.span));
                    }
                    if schema_names.contains(&variant.name)
                        || definition_names.contains(&variant.name)
                    {
                        return Err(Diagnostic::new(
                            "PROGRAM_DUPLICATE_BINDING",
                            "constructor, schema, and definition names must be unique",
                            json!({ "name": variant.name }),
                        )
                        .at(form_datum.span));
                    }
                }
                data_types.push(definition);
            }
            "signature" => {
                if signatures.len() >= MAXIMUM_SIGNATURES {
                    return Err(Diagnostic::new(
                        "PROGRAM_TOO_MANY_SIGNATURES",
                        "program declares too many function signatures",
                        json!({ "maximum": MAXIMUM_SIGNATURES }),
                    )
                    .at(form_datum.span));
                }
                let signature = parse_function_signature(form, form_datum)?;
                if !signature_names.insert(signature.name.clone()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_SIGNATURE",
                        "function signature name is not unique",
                        json!({ "name": signature.name }),
                    )
                    .at(form_datum.span));
                }
                signatures.push(signature);
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
                if constructor_names.contains(&schema_name) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_BINDING",
                        "constructor, schema, and definition names must be unique",
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
                if constructor_names.contains(&definition_name) {
                    return Err(Diagnostic::new(
                        "PROGRAM_DUPLICATE_BINDING",
                        "constructor, schema, and definition names must be unique",
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
            "export-types" => {
                if type_exports.is_some() {
                    return Err(at(
                        form_datum,
                        "PROGRAM_DUPLICATE_TYPE_EXPORT",
                        "program has multiple export-types forms",
                    ));
                }
                let values = symbols(&form[1..])
                    .filter(|values| !values.is_empty())
                    .ok_or_else(|| {
                        at(
                            form_datum,
                            "PROGRAM_INVALID_TYPE_EXPORT",
                            "export-types must contain at least one type name",
                        )
                    })?;
                ensure_unique(
                    &values,
                    "PROGRAM_DUPLICATE_TYPE_EXPORT_NAME",
                    "type export name is listed more than once",
                    form_datum,
                )?;
                type_exports = Some(values);
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
    if version > BigInt::from(MAXIMUM_LANGUAGE_VERSION) {
        return Err(Diagnostic::new(
            "PROGRAM_UNSUPPORTED_VERSION",
            "program requests an unsupported language version",
            json!({
                "actualVersion": version.to_string(),
                "minimumSupportedVersion": 1,
                "maximumSupportedVersion": MAXIMUM_LANGUAGE_VERSION,
            }),
        )
        .at(datum.span));
    }
    let exports = exports.ok_or_else(|| {
        Diagnostic::simple(
            "PROGRAM_MISSING_EXPORT",
            "program is missing an export form",
        )
        .at(datum.span)
    })?;
    let imports = imports.unwrap_or_default();
    let type_exports = type_exports.unwrap_or_default();
    if imports.iter().any(|import| import == &name) {
        return Err(Diagnostic::new(
            "PROGRAM_SELF_IMPORT",
            "program cannot import itself",
            json!({ "module": name }),
        )
        .at(datum.span));
    }
    if version < BigInt::from(3_u8) && !imports.is_empty() {
        return Err(program_feature_requires_version(
            "imports", &version, 3, datum,
        ));
    }
    if version < BigInt::from(3_u8) && !data_types.is_empty() {
        return Err(program_feature_requires_version("data", &version, 3, datum));
    }
    if version < BigInt::from(4_u8) && !signatures.is_empty() {
        return Err(program_feature_requires_version(
            "signature",
            &version,
            4,
            datum,
        ));
    }
    if version < BigInt::from(4_u8) && !type_exports.is_empty() {
        return Err(program_feature_requires_version(
            "export-types",
            &version,
            4,
            datum,
        ));
    }
    for type_name in &type_exports {
        if !data_type_names.contains(type_name) {
            return Err(Diagnostic::new(
                "PROGRAM_UNKNOWN_TYPE_EXPORT",
                "export-types name does not identify a local data type",
                json!({ "name": type_name }),
            )
            .at(datum.span));
        }
    }
    for data_type in &data_types {
        for variant in &data_type.variants {
            for field in &variant.fields {
                match (&field.type_expression, version >= BigInt::from(4_u8)) {
                    (Some(_), false) => {
                        return Err(program_feature_requires_version(
                            "typed-data-field",
                            &version,
                            4,
                            datum,
                        ));
                    }
                    (None, true) => {
                        return Err(Diagnostic::new(
                            "PROGRAM_DATA_FIELD_REQUIRES_TYPE",
                            "language version 4 data fields require explicit types",
                            json!({
                                "type": data_type.name,
                                "variant": variant.name,
                                "field": field.name,
                            }),
                        )
                        .at(datum.span));
                    }
                    (Some(type_expression), true) => {
                        ensure_known_type(
                            type_expression,
                            &data_type_names,
                            !imports.is_empty(),
                            datum,
                        )?;
                    }
                    _ => {}
                }
            }
        }
    }
    for signature in &signatures {
        if !definition_names.contains(&signature.name) {
            return Err(Diagnostic::new(
                "PROGRAM_UNKNOWN_SIGNATURE",
                "signature does not name a program definition",
                json!({ "name": signature.name }),
            )
            .at(datum.span));
        }
        for parameter in &signature.parameters {
            ensure_known_type(parameter, &data_type_names, !imports.is_empty(), datum)?;
        }
        ensure_known_type(
            &signature.result,
            &data_type_names,
            !imports.is_empty(),
            datum,
        )?;
    }
    for export_name in &exports {
        if !definition_names.contains(export_name) && !constructor_names.contains(export_name) {
            return Err(Diagnostic::new(
                "PROGRAM_UNKNOWN_EXPORT",
                "export does not name a program definition or data constructor",
                json!({ "name": export_name }),
            )
            .at(datum.span));
        }
    }
    if version >= BigInt::from(4_u8) {
        for export_name in &exports {
            if definition_names.contains(export_name) && !signature_names.contains(export_name) {
                return Err(Diagnostic::new(
                    "PROGRAM_EXPORT_REQUIRES_SIGNATURE",
                    "language version 4 exported definitions require a function signature",
                    json!({ "name": export_name }),
                )
                .at(datum.span));
            }
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
    for schema in &schemas {
        ensure_schema_version(&schema.kind, &version, datum)?;
    }
    for definition in &definitions {
        ensure_expression_version(&definition.expression, &version)?;
    }
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
        imports,
        schemas,
        data_types,
        signatures,
        type_exports,
        routes,
        definitions,
        exports,
        source: source.to_owned(),
    })
}

fn parse_data_type(form: &[Datum], datum: &Datum) -> AilResult<DataTypeDefinition> {
    if form.len() < 3 || form[1].symbol().is_none() {
        return Err(at(
            datum,
            "PROGRAM_INVALID_DATA_TYPE",
            "data type must be (data name (constructor field ... ) ...)",
        ));
    }
    if form.len() - 2 > MAXIMUM_DATA_VARIANTS {
        return Err(Diagnostic::new(
            "PROGRAM_TOO_MANY_DATA_VARIANTS",
            "data type declares too many variants",
            json!({ "maximum": MAXIMUM_DATA_VARIANTS }),
        )
        .at(datum.span));
    }
    let mut variants = Vec::with_capacity(form.len() - 2);
    let mut names = HashSet::new();
    for raw_variant in &form[2..] {
        let variant = raw_variant.list().unwrap_or_default();
        let Some(name) = variant.first().and_then(Datum::symbol) else {
            return Err(Diagnostic::new(
                "PROGRAM_INVALID_DATA_VARIANT",
                "data variant must be (constructor field ...)",
                json!({ "variant": raw_variant.display() }),
            )
            .at(raw_variant.span));
        };
        if variant.len().saturating_sub(1) > MAXIMUM_VARIANT_FIELDS {
            return Err(Diagnostic::new(
                "PROGRAM_TOO_MANY_VARIANT_FIELDS",
                "data variant declares too many fields",
                json!({ "maximum": MAXIMUM_VARIANT_FIELDS }),
            )
            .at(raw_variant.span));
        }
        if !names.insert(name.to_owned()) {
            return Err(Diagnostic::new(
                "PROGRAM_DUPLICATE_DATA_VARIANT",
                "data variant name is not unique within its type",
                json!({ "name": name }),
            )
            .at(raw_variant.span));
        }
        let fields = variant[1..]
            .iter()
            .map(parse_data_field)
            .collect::<AilResult<Vec<_>>>()?;
        let field_names = fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>();
        ensure_unique(
            &field_names,
            "PROGRAM_DUPLICATE_DATA_FIELD",
            "data variant field name is not unique",
            raw_variant,
        )?;
        variants.push(VariantDefinition {
            name: name.to_owned(),
            fields,
        });
    }
    Ok(DataTypeDefinition {
        name: form[1].symbol().unwrap_or_default().to_owned(),
        variants,
    })
}

fn parse_data_field(datum: &Datum) -> AilResult<DataField> {
    if let Some(name) = datum.symbol() {
        return Ok(DataField {
            name: name.to_owned(),
            type_expression: None,
        });
    }
    let field = datum.list().unwrap_or_default();
    if field.len() != 2 || field[0].symbol().is_none() {
        return Err(Diagnostic::new(
            "PROGRAM_INVALID_DATA_FIELD",
            "data field must be a name or (name type)",
            json!({ "field": datum.display() }),
        )
        .at(datum.span));
    }
    Ok(DataField {
        name: field[0].symbol().unwrap_or_default().to_owned(),
        type_expression: Some(parse_type_expression(&field[1], 0)?),
    })
}

fn parse_function_signature(form: &[Datum], datum: &Datum) -> AilResult<FunctionSignature> {
    if form.len() != 3 || form[1].symbol().is_none() {
        return Err(at(
            datum,
            "PROGRAM_INVALID_SIGNATURE",
            "signature must be (signature name (fn (parameter-type ...) result-type))",
        ));
    }
    let type_expression = parse_type_expression(&form[2], 0)?;
    let TypeExpression::Function { parameters, result } = type_expression else {
        return Err(at(
            datum,
            "PROGRAM_INVALID_SIGNATURE",
            "signature type must be a function type",
        ));
    };
    Ok(FunctionSignature {
        name: form[1].symbol().unwrap_or_default().to_owned(),
        parameters,
        result: *result,
    })
}

fn parse_type_expression(datum: &Datum, depth: usize) -> AilResult<TypeExpression> {
    if depth > MAXIMUM_TYPE_DEPTH {
        return Err(Diagnostic::new(
            "PROGRAM_TYPE_TOO_DEEP",
            "type expression exceeds the maximum nesting depth",
            json!({ "maximum": MAXIMUM_TYPE_DEPTH }),
        )
        .at(datum.span));
    }
    if let Some(name) = datum.symbol() {
        return Ok(TypeExpression::Named(name.to_owned()));
    }
    let form = datum.list().unwrap_or_default();
    match form.first().and_then(Datum::symbol) {
        Some("list") if form.len() == 2 => Ok(TypeExpression::List(Box::new(
            parse_type_expression(&form[1], depth + 1)?,
        ))),
        Some("result") if form.len() == 3 => Ok(TypeExpression::Result {
            success: Box::new(parse_type_expression(&form[1], depth + 1)?),
            error: Box::new(parse_type_expression(&form[2], depth + 1)?),
        }),
        Some("fn") if form.len() == 3 => {
            let parameters = form[1].list().ok_or_else(|| {
                at(
                    &form[1],
                    "PROGRAM_INVALID_TYPE",
                    "function type parameters must be a proper list",
                )
            })?;
            Ok(TypeExpression::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| parse_type_expression(parameter, depth + 1))
                    .collect::<AilResult<Vec<_>>>()?,
                result: Box::new(parse_type_expression(&form[2], depth + 1)?),
            })
        }
        _ => Err(Diagnostic::new(
            "PROGRAM_INVALID_TYPE",
            "unknown or malformed type expression",
            json!({ "type": datum.display() }),
        )
        .at(datum.span)),
    }
}

fn ensure_known_type(
    type_expression: &TypeExpression,
    data_types: &HashSet<String>,
    allow_external: bool,
    datum: &Datum,
) -> AilResult<()> {
    match type_expression {
        TypeExpression::Named(name) => {
            const BUILTINS: &[&str] = &[
                "any", "integer", "boolean", "string", "symbol", "nil", "map",
            ];
            if BUILTINS.contains(&name.as_str()) || data_types.contains(name) || allow_external {
                Ok(())
            } else {
                Err(Diagnostic::new(
                    "PROGRAM_UNKNOWN_TYPE",
                    "type expression names an unknown type",
                    json!({ "name": name }),
                )
                .at(datum.span))
            }
        }
        TypeExpression::List(item) => ensure_known_type(item, data_types, allow_external, datum),
        TypeExpression::Result { success, error } => {
            ensure_known_type(success, data_types, allow_external, datum)?;
            ensure_known_type(error, data_types, allow_external, datum)
        }
        TypeExpression::Function { parameters, result } => {
            for parameter in parameters {
                ensure_known_type(parameter, data_types, allow_external, datum)?;
            }
            ensure_known_type(result, data_types, allow_external, datum)
        }
    }
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
        Some("enum") => {
            if !(2..=MAXIMUM_ENUM_VALUES + 1).contains(&form.len()) {
                return Err(Diagnostic::new(
                    "PROGRAM_INVALID_SCHEMA_SPECIFICATION",
                    "enum must contain between 1 and 64 unique scalar values",
                    json!({ "schema": datum.display(), "maximum": MAXIMUM_ENUM_VALUES }),
                )
                .at(datum.span));
            }
            let mut values = Vec::with_capacity(form.len() - 1);
            let mut seen = HashSet::new();
            for value in &form[1..] {
                if !matches!(
                    &value.kind,
                    DatumKind::Integer(_) | DatumKind::Bool(_) | DatumKind::String(_)
                ) {
                    return Err(invalid_schema(
                        datum,
                        "enum values must be integer, boolean, or string literals",
                    ));
                }
                if !seen.insert(value.display()) {
                    return Err(Diagnostic::new(
                        "PROGRAM_SCHEMA_DUPLICATE_ENUM_VALUE",
                        "enum value is not unique",
                        json!({ "value": value.display() }),
                    )
                    .at(value.span));
                }
                values.push(value.clone());
            }
            Ok(SchemaKind::Enum { values })
        }
        Some("union") => {
            let variant_count = form.len().saturating_sub(1);
            if !(2..=MAXIMUM_UNION_VARIANTS).contains(&variant_count) {
                return Err(Diagnostic::new(
                    "PROGRAM_INVALID_SCHEMA_SPECIFICATION",
                    "union must contain between 2 and 8 schema variants",
                    json!({ "schema": datum.display(), "maximum": MAXIMUM_UNION_VARIANTS }),
                )
                .at(datum.span));
            }
            Ok(SchemaKind::Union {
                variants: form[1..]
                    .iter()
                    .map(|variant| parse_schema_specification(variant, depth + 1))
                    .collect::<AilResult<Vec<_>>>()?,
            })
        }
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
                Some("and") => ExpressionKind::And(
                    form[1..]
                        .iter()
                        .map(parse_expression)
                        .collect::<AilResult<Vec<_>>>()?,
                ),
                Some("or") => ExpressionKind::Or(
                    form[1..]
                        .iter()
                        .map(parse_expression)
                        .collect::<AilResult<Vec<_>>>()?,
                ),
                Some("cond") => parse_cond(form, datum)?,
                Some("match") => parse_match(form, datum)?,
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

fn parse_cond(form: &[Datum], datum: &Datum) -> AilResult<ExpressionKind> {
    let raw_clauses = &form[1..];
    let Some(raw_alternative) = raw_clauses.last() else {
        return Err(Diagnostic::simple(
            "PARSE_COND_MISSING_ELSE",
            "cond must end with an explicit else clause",
        )
        .at(datum.span));
    };
    let alternative = raw_alternative.list().unwrap_or_default();
    if alternative.len() != 2 || alternative[0].symbol() != Some("else") {
        return Err(Diagnostic::simple(
            "PARSE_COND_MISSING_ELSE",
            "cond must end with an explicit else clause",
        )
        .at(raw_alternative.span));
    }

    let mut clauses = Vec::with_capacity(raw_clauses.len().saturating_sub(1));
    for raw_clause in &raw_clauses[..raw_clauses.len().saturating_sub(1)] {
        let clause = raw_clause.list().unwrap_or_default();
        if clause.len() != 2 || clause[0].symbol() == Some("else") {
            return Err(Diagnostic::new(
                "PARSE_INVALID_COND_CLAUSE",
                "cond clauses must be (condition expression), with else only at the end",
                json!({ "clause": raw_clause.display() }),
            )
            .at(raw_clause.span));
        }
        clauses.push(CondClause {
            condition: parse_expression(&clause[0])?,
            expression: parse_expression(&clause[1])?,
        });
    }
    Ok(ExpressionKind::Cond {
        clauses,
        alternative: Box::new(parse_expression(&alternative[1])?),
    })
}

fn parse_match(form: &[Datum], datum: &Datum) -> AilResult<ExpressionKind> {
    if form.len() < 3 {
        return Err(Diagnostic::simple(
            "PARSE_MATCH_MISSING_DEFAULT",
            "match must contain a value and end with an explicit _ arm",
        )
        .at(datum.span));
    }
    let raw_arms = &form[2..];
    let final_arm = raw_arms
        .last()
        .and_then(Datum::list)
        .filter(|arm| arm.len() == 2);
    if final_arm.and_then(|arm| arm[0].symbol()) != Some("_") {
        return Err(Diagnostic::simple(
            "PARSE_MATCH_MISSING_DEFAULT",
            "match must end with an explicit _ arm",
        )
        .at(raw_arms.last().map_or(datum.span, |arm| arm.span)));
    }

    let mut arms = Vec::with_capacity(raw_arms.len());
    for (index, raw_arm) in raw_arms.iter().enumerate() {
        let arm = raw_arm.list().unwrap_or_default();
        if arm.len() != 2 {
            return Err(Diagnostic::new(
                "PARSE_INVALID_MATCH_ARM",
                "match arm must be (pattern expression)",
                json!({ "arm": raw_arm.display() }),
            )
            .at(raw_arm.span));
        }
        if arm[0].symbol() == Some("_") && index + 1 != raw_arms.len() {
            return Err(Diagnostic::simple(
                "PARSE_MATCH_DEFAULT_NOT_LAST",
                "the _ match arm must be last",
            )
            .at(raw_arm.span));
        }
        let mut bindings = HashSet::new();
        arms.push(MatchArm {
            pattern: parse_pattern(&arm[0], &mut bindings)?,
            expression: parse_expression(&arm[1])?,
        });
    }
    Ok(ExpressionKind::Match {
        value: Box::new(parse_expression(&form[1])?),
        arms,
    })
}

fn parse_pattern(datum: &Datum, bindings: &mut HashSet<String>) -> AilResult<Pattern> {
    let kind = match &datum.kind {
        DatumKind::Symbol(name) if name == "_" => PatternKind::Wildcard,
        DatumKind::Symbol(name) => {
            if !bindings.insert(name.clone()) {
                return Err(Diagnostic::new(
                    "PARSE_DUPLICATE_PATTERN_BINDING",
                    "pattern binding name is not unique",
                    json!({ "name": name }),
                )
                .at(datum.span));
            }
            PatternKind::Binding(name.clone())
        }
        DatumKind::Integer(_) | DatumKind::Bool(_) | DatumKind::String(_) => {
            PatternKind::Literal(datum.clone())
        }
        DatumKind::List(items) if !items.is_empty() => {
            let Some(name) = items[0].symbol() else {
                return Err(Diagnostic::new(
                    "PARSE_INVALID_PATTERN",
                    "variant pattern must begin with a constructor name",
                    json!({ "pattern": datum.display() }),
                )
                .at(datum.span));
            };
            PatternKind::Variant {
                name: name.to_owned(),
                fields: items[1..]
                    .iter()
                    .map(|item| parse_pattern(item, bindings))
                    .collect::<AilResult<Vec<_>>>()?,
            }
        }
        DatumKind::List(_) => {
            return Err(Diagnostic::simple(
                "PARSE_INVALID_PATTERN",
                "empty list is not a match pattern",
            )
            .at(datum.span));
        }
    };
    Ok(Pattern {
        kind,
        span: datum.span,
    })
}

fn ensure_expression_version(expression: &Expression, version: &BigInt) -> AilResult<()> {
    let minimum = match &expression.kind {
        ExpressionKind::And(_) => Some(("and", 2_u8)),
        ExpressionKind::Or(_) => Some(("or", 2_u8)),
        ExpressionKind::Cond { .. } => Some(("cond", 2_u8)),
        ExpressionKind::Match { .. } => Some(("match", 3_u8)),
        _ => None,
    };
    if let Some((feature, minimum_version)) = minimum
        && version < &BigInt::from(minimum_version)
    {
        return Err(Diagnostic::new(
            "PROGRAM_FEATURE_REQUIRES_VERSION",
            "program uses a feature from a newer language version",
            json!({
                "feature": feature,
                "actualVersion": version.to_u64().unwrap_or_default(),
                "minimumVersion": minimum_version,
            }),
        )
        .at(expression.span));
    }

    match &expression.kind {
        ExpressionKind::If {
            condition,
            consequent,
            alternative,
        } => {
            ensure_expression_version(condition, version)?;
            ensure_expression_version(consequent, version)?;
            ensure_expression_version(alternative, version)
        }
        ExpressionKind::And(expressions)
        | ExpressionKind::Or(expressions)
        | ExpressionKind::Do(expressions) => {
            for item in expressions {
                ensure_expression_version(item, version)?;
            }
            Ok(())
        }
        ExpressionKind::Cond {
            clauses,
            alternative,
        } => {
            for clause in clauses {
                ensure_expression_version(&clause.condition, version)?;
                ensure_expression_version(&clause.expression, version)?;
            }
            ensure_expression_version(alternative, version)
        }
        ExpressionKind::Match { value, arms } => {
            ensure_expression_version(value, version)?;
            for arm in arms {
                ensure_expression_version(&arm.expression, version)?;
            }
            Ok(())
        }
        ExpressionKind::Let { bindings, body } => {
            for binding in bindings {
                ensure_expression_version(&binding.expression, version)?;
            }
            ensure_expression_version(body, version)
        }
        ExpressionKind::Function { body, .. } => ensure_expression_version(body, version),
        ExpressionKind::Call { callee, arguments } => {
            ensure_expression_version(callee, version)?;
            for argument in arguments {
                ensure_expression_version(argument, version)?;
            }
            Ok(())
        }
        ExpressionKind::Literal(_) | ExpressionKind::Variable(_) | ExpressionKind::Quote(_) => {
            Ok(())
        }
    }
}

fn program_feature_requires_version(
    feature: &str,
    version: &BigInt,
    minimum_version: u8,
    datum: &Datum,
) -> Diagnostic {
    Diagnostic::new(
        "PROGRAM_FEATURE_REQUIRES_VERSION",
        "program uses a feature from a newer language version",
        json!({
            "feature": feature,
            "actualVersion": version.to_u64().unwrap_or_default(),
            "minimumVersion": minimum_version,
        }),
    )
    .at(datum.span)
}

fn ensure_schema_version(schema: &SchemaKind, version: &BigInt, datum: &Datum) -> AilResult<()> {
    let feature = match schema {
        SchemaKind::Enum { .. } => Some("enum"),
        SchemaKind::Union { .. } => Some("union"),
        _ => None,
    };
    if let Some(feature) = feature
        && version < &BigInt::from(2_u8)
    {
        return Err(Diagnostic::new(
            "PROGRAM_FEATURE_REQUIRES_VERSION",
            "program uses a feature from a newer language version",
            json!({
                "feature": feature,
                "actualVersion": version.to_u64().unwrap_or_default(),
                "minimumVersion": 2,
            }),
        )
        .at(datum.span));
    }
    match schema {
        SchemaKind::Union { variants } => {
            for variant in variants {
                ensure_schema_version(variant, version, datum)?;
            }
        }
        SchemaKind::List { item, .. } => ensure_schema_version(item, version, datum)?,
        SchemaKind::Object { fields } => {
            for field in fields {
                ensure_schema_version(&field.specification, version, datum)?;
            }
        }
        SchemaKind::Any
        | SchemaKind::Enum { .. }
        | SchemaKind::String { .. }
        | SchemaKind::Integer { .. }
        | SchemaKind::Boolean => {}
    }
    Ok(())
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
        (SchemaKind::Enum { values }, _) => values.iter().any(|value| value.kind == datum.kind),
        (SchemaKind::Union { variants }, _) => variants
            .iter()
            .any(|variant| schema_accepts(variant, datum)),
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

    use crate::{TypeExpression, load_program_source};

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

    #[test]
    fn gates_v2_forms_and_unknown_versions() {
        let v1 = require_error(load_program_source(
            "(program (name old) (version 1) (def run (and #t #t)) (export run))",
        ));
        assert_eq!(v1.code, "PROGRAM_FEATURE_REQUIRES_VERSION");
        assert_eq!(
            v1.details.as_ref(),
            &json!({ "feature": "and", "actualVersion": 1, "minimumVersion": 2 })
        );

        let future = require_error(load_program_source(
            "(program (name future) (version 5) (def run #t) (export run))",
        ));
        assert_eq!(future.code, "PROGRAM_UNSUPPORTED_VERSION");
        assert_eq!(
            future.details.as_ref(),
            &json!({
                "actualVersion": "5",
                "minimumSupportedVersion": 1,
                "maximumSupportedVersion": 4,
            })
        );
    }

    #[test]
    fn requires_an_explicit_final_cond_else_clause() {
        let diagnostic = require_error(load_program_source(
            "(program (name rules) (version 2) (def run (cond (#t 1))) (export run))",
        ));
        assert_eq!(diagnostic.code, "PARSE_COND_MISSING_ELSE");

        let program = load_program_source(
            "(program (name rules) (version 2) (def run (cond (#f 1) (else 2))) (export run))",
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            program.definitions[0].expression.to_json(),
            json!({
                "type": "cond",
                "clauses": [{
                    "condition": { "type": "literal", "value": false },
                    "expression": { "type": "literal", "value": 1 },
                }],
                "alternative": { "type": "literal", "value": 2 },
            })
        );
    }

    #[test]
    fn parses_and_versions_enum_and_union_schemas() {
        let old = require_error(load_program_source(
            r#"(program
                (name old-schema)
                (version 1)
                (schema action (enum "approve" "reject"))
                (def run #t)
                (export run))"#,
        ));
        assert_eq!(old.code, "PROGRAM_FEATURE_REQUIRES_VERSION");
        assert_eq!(
            old.details.as_ref(),
            &json!({ "feature": "enum", "actualVersion": 1, "minimumVersion": 2 })
        );

        let program = load_program_source(
            r#"(program
                (name decisions)
                (version 2)
                (schema action (enum "approve" "reject"))
                (schema identity (union integer string))
                (def run #t)
                (export run))"#,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let schemas = program
            .inspect_json()
            .get("schemas")
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            schemas,
            json!([
                {
                    "name": "action",
                    "schema": { "type": "enum", "values": ["approve", "reject"] },
                },
                {
                    "name": "identity",
                    "schema": {
                        "type": "union",
                        "variants": [
                            { "type": "integer", "minimum": false, "maximum": false },
                            {
                                "type": "string",
                                "minimumLength": 0,
                                "maximumLength": false,
                            },
                        ],
                    },
                },
            ])
        );
    }

    #[test]
    fn parses_v3_imports_data_types_and_total_match() {
        let program = load_program_source(
            r#"(program
                (name approvals)
                (version 3)
                (imports policy)
                (data decision (approved id) (rejected reason))
                (def describe (fn (value)
                  (match value
                    ((approved id) id)
                    ((rejected reason) reason)
                    (_ "unknown"))))
                (export describe))"#,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(program.imports, ["policy"]);
        assert_eq!(program.data_types[0].name, "decision");
        assert_eq!(program.data_types[0].variants[0].fields[0].name, "id");
        assert!(
            program.data_types[0].variants[0].fields[0]
                .type_expression
                .is_none()
        );

        let missing_default = require_error(load_program_source(
            r#"(program
                (name invalid)
                (version 3)
                (def run (fn (value) (match value (1 "one"))))
                (export run))"#,
        ));
        assert_eq!(missing_default.code, "PARSE_MATCH_MISSING_DEFAULT");
    }

    #[test]
    fn gates_v3_program_features() {
        let imports = require_error(load_program_source(
            "(program (name old) (version 2) (imports helper) (def run #t) (export run))",
        ));
        assert_eq!(imports.code, "PROGRAM_FEATURE_REQUIRES_VERSION");
        assert_eq!(
            imports.details.as_ref(),
            &json!({ "feature": "imports", "actualVersion": 2, "minimumVersion": 3 })
        );

        let data = require_error(load_program_source(
            "(program (name old) (version 2) (data maybe (some value) (none)) (def run #t) (export run))",
        ));
        assert_eq!(data.code, "PROGRAM_FEATURE_REQUIRES_VERSION");

        let matcher = require_error(load_program_source(
            "(program (name old) (version 2) (def run (match 1 (_ #t))) (export run))",
        ));
        assert_eq!(matcher.code, "PROGRAM_FEATURE_REQUIRES_VERSION");
    }

    #[test]
    fn parses_and_gates_v4_types_and_signatures() {
        let program = load_program_source(
            r#"(program
                (name typed-decisions)
                (version 4)
                (data decision
                  (approved (amount integer))
                  (rejected (reason string)))
                (export-types decision)
                (signature decide (fn (integer) decision))
                (def decide (fn (amount) (approved amount)))
                (export decide approved rejected))"#,
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(program.signatures[0].name, "decide");
        assert_eq!(program.type_exports, ["decision"]);
        assert_eq!(
            program.data_types[0].variants[0].fields[0]
                .type_expression
                .as_ref()
                .map(TypeExpression::to_json),
            Some(json!({ "type": "named", "name": "integer" }))
        );

        let untyped = require_error(load_program_source(
            "(program (name invalid) (version 4) (data maybe (some value) (none)) (def run (fn () (none))) (signature run (fn () maybe)) (export run))",
        ));
        assert_eq!(untyped.code, "PROGRAM_DATA_FIELD_REQUIRES_TYPE");

        let missing_signature = require_error(load_program_source(
            "(program (name invalid) (version 4) (def run (fn () 1)) (export run))",
        ));
        assert_eq!(missing_signature.code, "PROGRAM_EXPORT_REQUIRES_SIGNATURE");

        let old = require_error(load_program_source(
            "(program (name old) (version 3) (signature run (fn () integer)) (def run (fn () 1)) (export run))",
        ));
        assert_eq!(old.code, "PROGRAM_FEATURE_REQUIRES_VERSION");

        let old_type_export = require_error(load_program_source(
            "(program (name old) (version 3) (data maybe (some value)) (export-types maybe) (def run (fn () 1)) (export run))",
        ));
        assert_eq!(old_type_export.code, "PROGRAM_FEATURE_REQUIRES_VERSION");

        let unknown_type_export = require_error(load_program_source(
            "(program (name invalid) (version 4) (export-types missing) (signature run (fn () integer)) (def run (fn () 1)) (export run))",
        ));
        assert_eq!(unknown_type_export.code, "PROGRAM_UNKNOWN_TYPE_EXPORT");

        let imported_type = load_program_source(
            "(program (name app) (version 4) (imports model) (signature run (fn (external) external)) (def run (fn (value) value)) (export run))",
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        assert_eq!(
            imported_type.signatures[0].parameters[0].to_json(),
            json!({ "type": "named", "name": "external" })
        );
    }
}
