pub(crate) mod catalog;
mod token;

use catalog::{FormContext, FormHelp, PrimitiveHelp, form_help, primitive_help};
use token::{SymbolToken, symbol_at};
use yanshu_analysis::{AnalysisReport, Type};
use yanshu_diagnostic::Span;
use yanshu_library::{FuelModel, LibraryContract, OperationContract, trusted_contract};
use yanshu_syntax::{
    ExpressionKind, ExpressionNode, Program, ReaderLimits, SchemaKind, SymbolBinding,
    SymbolBindingKind, expression_nodes, read_source, symbol_index,
};

pub(crate) const MAXIMUM_HOVER_TEXT_BYTES: usize = 8 * 1024;
const MAXIMUM_HOVER_JSON_OVERHEAD_BYTES: usize = 1024;

const _: () = assert!(
    MAXIMUM_HOVER_TEXT_BYTES * 6 + MAXIMUM_HOVER_JSON_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_LSP_MESSAGE_BYTES
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HoverResult {
    pub(crate) text: String,
    pub(crate) span: Span,
}

pub(crate) fn hover_at(
    program: &Program,
    analysis: Option<&AnalysisReport>,
    offset: usize,
) -> Option<HoverResult> {
    let root = read_source(&program.source, ReaderLimits::default()).ok()?;
    let token = symbol_at(&root, offset)?;
    let version = program.version.to_string().parse::<u8>().ok()?;
    let nodes = expression_nodes(program);

    if let Some(entry) = expression_form_for(&token, &nodes) {
        return finish(
            render_form(entry, version, node_at(&nodes, token.span.start.offset)),
            token.span,
        );
    }
    if let Some(entry) = structural_form_for(&token) {
        return finish(render_form(entry, version, None), token.span);
    }

    // Scope resolution must succeed before a variable can be called a primitive.
    // Otherwise a malformed index could make a shadowed name look host-owned.
    let index = symbol_index(program).ok()?;
    if let Some(binding) = binding_at(index.bindings(), offset) {
        return finish(
            render_binding(
                program,
                analysis,
                binding,
                node_at(&nodes, token.span.start.offset),
            ),
            token.span,
        );
    }

    let node = nodes.iter().find(|node| node.span == token.span)?;
    if let Some(entry) = primitive_help(&token.name) {
        return finish(
            render_primitive(program, entry, version, Some(node)),
            token.span,
        );
    }
    if let Some((contract, operation, declared)) = library_operation(program, &token.name) {
        return finish(
            render_library(&token.name, contract, operation, declared, Some(node)),
            token.span,
        );
    }
    if let Some(text) = render_constructor(program, &token.name, Some(node)) {
        return finish(text, token.span);
    }
    if let Some(text) = render_schema(program, &token.name, Some(node)) {
        return finish(text, token.span);
    }
    None
}

fn expression_form_for(token: &SymbolToken, nodes: &[ExpressionNode]) -> Option<FormHelp> {
    if !token.is_head || !nodes.iter().any(|node| node.span == token.parent_span) {
        return None;
    }
    form_help(FormContext::Expression, &token.name)
}

fn structural_form_for(token: &SymbolToken) -> Option<FormHelp> {
    if !token.is_head {
        return None;
    }
    if token.parent_depth == 0 && token.name == "program" {
        return form_help(FormContext::TopLevel, &token.name);
    }
    if token.parent_depth == 1 {
        return form_help(FormContext::TopLevel, &token.name);
    }
    match token.top_level_form.as_deref() {
        Some("signature") => form_help(FormContext::Type, &token.name),
        Some("schema") => form_help(FormContext::Schema, &token.name),
        _ => None,
    }
}

fn binding_at(bindings: &[SymbolBinding], offset: usize) -> Option<&SymbolBinding> {
    bindings.iter().find(|binding| {
        contains_offset(binding.declaration, offset)
            || binding
                .references
                .iter()
                .any(|reference| contains_offset(*reference, offset))
    })
}

fn render_form(entry: FormHelp, current_version: u8, node: Option<&ExpressionNode>) -> String {
    let mut text = format!(
        "{}\nkind: {}\nsince: language v{}\nsyntax: {}\n{}",
        entry.name, entry.kind, entry.minimum_version, entry.syntax, entry.summary
    );
    append_version_availability(&mut text, current_version, entry.minimum_version);
    append_node(&mut text, node);
    text
}

fn render_binding(
    program: &Program,
    analysis: Option<&AnalysisReport>,
    binding: &SymbolBinding,
    node: Option<&ExpressionNode>,
) -> String {
    let mut text = format!("{}\nkind: {}", binding.name, binding_kind(program, binding));
    if binding.kind == SymbolBindingKind::Definition {
        if let Some(value_type) = definition_type(program, analysis, &binding.name) {
            text.push_str("\ntype: ");
            text.push_str(&value_type.display());
        }
        if let Some(definition) = analysis.and_then(|report| {
            report
                .definitions
                .iter()
                .find(|definition| definition.name == binding.name)
        }) {
            text.push_str("\neffects: ");
            if definition.capabilities.is_empty() {
                text.push_str("pure");
            } else {
                text.push_str(&definition.capabilities.join(", "));
            }
        }
    } else {
        text.push_str("\nscope: lexical");
    }
    append_node(&mut text, node);
    text
}

fn binding_kind(program: &Program, binding: &SymbolBinding) -> &'static str {
    match binding.kind {
        SymbolBindingKind::Definition => {
            if program
                .definitions
                .iter()
                .find(|definition| definition.name == binding.name)
                .is_some_and(|definition| {
                    matches!(definition.expression.kind, ExpressionKind::Function { .. })
                })
            {
                "function definition"
            } else {
                "global definition"
            }
        }
        SymbolBindingKind::Parameter => "function parameter",
        SymbolBindingKind::Let => "let binding",
        SymbolBindingKind::Pattern => "pattern binding",
    }
}

fn definition_type(
    program: &Program,
    analysis: Option<&AnalysisReport>,
    name: &str,
) -> Option<Type> {
    analysis
        .and_then(|report| {
            report
                .definitions
                .iter()
                .find(|definition| definition.name == name)
                .map(|definition| definition.inferred_type.clone())
        })
        .or_else(|| {
            program
                .signatures
                .iter()
                .find(|signature| signature.name == name)
                .map(|signature| Type::Function {
                    parameters: signature
                        .parameters
                        .iter()
                        .map(Type::from_expression)
                        .collect(),
                    result: Box::new(Type::from_expression(&signature.result)),
                })
        })
}

fn render_primitive(
    program: &Program,
    entry: PrimitiveHelp,
    current_version: u8,
    node: Option<&ExpressionNode>,
) -> String {
    let mut text = format!(
        "{}\nkind: core primitive\nsince: language v{}\ntype: {}\neffects: {}\n{}",
        entry.name, entry.minimum_version, entry.signature, entry.effects, entry.summary
    );
    append_version_availability(&mut text, current_version, entry.minimum_version);
    if let Some(requirement) = entry.requirement {
        text.push_str("\nrequires: ");
        text.push_str(requirement);
        if entry.effects != "pure"
            && !program
                .capabilities
                .iter()
                .any(|item| item == entry.effects)
        {
            text.push_str(" (capability is not declared by this program)");
        }
    }
    if let Some(metering) = entry.metering {
        text.push_str("\nfuel: ");
        text.push_str(metering);
    }
    append_node(&mut text, node);
    text
}

fn append_version_availability(text: &mut String, actual: u8, minimum: u8) {
    if actual < minimum {
        text.push_str(&format!(
            "\navailability: unavailable in language v{actual}; requires v{minimum}"
        ));
    }
}

fn library_operation(
    program: &Program,
    public_name: &str,
) -> Option<(LibraryContract, OperationContract, bool)> {
    let (library, operation_name) = public_name.split_once('/')?;
    if let Some(requirement) = program
        .libraries
        .iter()
        .find(|requirement| requirement.name == library)
    {
        let contract = trusted_contract(library, requirement.version)?;
        return contract
            .operation(operation_name)
            .map(|operation| (contract, operation, true));
    }
    let contract = trusted_contract(library, 1)?;
    contract
        .operation(operation_name)
        .map(|operation| (contract, operation, false))
}

fn render_library(
    public_name: &str,
    contract: LibraryContract,
    operation: OperationContract,
    declared: bool,
    node: Option<&ExpressionNode>,
) -> String {
    let parameters = operation
        .parameters
        .iter()
        .map(|parameter| parameter.display())
        .collect::<Vec<_>>()
        .join(", ");
    let declaration = if declared { "declared" } else { "not declared" };
    let mut text = format!(
        "{public_name}\nkind: library operation\ntype: fn({parameters}) -> {}\neffects: pure\nlibrary: {}@{} ({declaration})\nfuel: {}\n{}",
        operation.result.display(),
        contract.name,
        contract.version,
        fuel_description(operation.fuel),
        library_summary(operation.name),
    );
    append_node(&mut text, node);
    text
}

fn fuel_description(model: FuelModel) -> String {
    match model {
        FuelModel::Fixed(value) => format!("{value} per call"),
        FuelModel::TextCharacters { base, block_size } => {
            format!("{base} + ceil(total Unicode scalar count / {block_size})")
        }
        FuelModel::TextReplace { base, block_size } => {
            format!("{base} + ceil(text/replacement work and output bytes / {block_size})")
        }
    }
}

fn library_summary(operation: &str) -> &'static str {
    match operation {
        "length" => "Counts Unicode scalar values rather than UTF-8 bytes.",
        "starts-with?" => "Tests a Unicode string prefix through the declared text@1 backend.",
        "ends-with?" => "Tests a Unicode string suffix through the declared text@1 backend.",
        "contains?" => "Tests substring containment through the declared text@1 backend.",
        "replace" => "Replaces all matches after checking bounded output amplification.",
        _ => "Invokes one operation from a trusted versioned Library Backend contract.",
    }
}

fn render_constructor(
    program: &Program,
    name: &str,
    node: Option<&ExpressionNode>,
) -> Option<String> {
    let (data_type, variant) = program.data_types.iter().find_map(|data_type| {
        data_type
            .variants
            .iter()
            .find(|variant| variant.name == name)
            .map(|variant| (data_type, variant))
    })?;
    let value_type = Type::Function {
        parameters: variant
            .fields
            .iter()
            .map(|field| {
                field
                    .type_expression
                    .as_ref()
                    .map_or(Type::Any, Type::from_expression)
            })
            .collect(),
        result: Box::new(Type::User(data_type.name.clone())),
    };
    let mut text = format!(
        "{name}\nkind: data constructor\nsince: language v3\ntype: {}\neffects: pure\nConstructs one closed {} variant with fields in declaration order.",
        value_type.display(),
        data_type.name,
    );
    append_node(&mut text, node);
    Some(text)
}

fn render_schema(program: &Program, name: &str, node: Option<&ExpressionNode>) -> Option<String> {
    let schema = program.schemas.iter().find(|schema| schema.name == name)?;
    let mut text = format!(
        "{name}\nkind: schema value\neffects: pure\n{}",
        schema_summary(&schema.kind)
    );
    append_node(&mut text, node);
    Some(text)
}

fn schema_summary(kind: &SchemaKind) -> &'static str {
    match kind {
        SchemaKind::Any => "Accepts any bounded portable guest value.",
        SchemaKind::Enum { .. } => "Accepts one value from a closed scalar enumeration.",
        SchemaKind::Union { .. } => "Tries a bounded ordered union of schema alternatives.",
        SchemaKind::String { .. } => "Validates a string and its configured scalar-length bounds.",
        SchemaKind::Integer { .. } => {
            "Validates an arbitrary-precision integer and its configured bounds."
        }
        SchemaKind::Boolean => "Accepts only #t or #f.",
        SchemaKind::List { .. } => "Validates a bounded list and every item in source order.",
        SchemaKind::Object { .. } => {
            "Validates declared fields, rejects extras, and normalizes defaults."
        }
    }
}

fn node_at(nodes: &[ExpressionNode], offset: usize) -> Option<&ExpressionNode> {
    nodes
        .iter()
        .filter(|node| contains_offset(node.span, offset))
        .min_by_key(|node| node.span.end.offset.saturating_sub(node.span.start.offset))
}

fn append_node(text: &mut String, node: Option<&ExpressionNode>) {
    if let Some(node) = node {
        text.push_str("\nnode: ");
        text.push_str(&node.id);
    }
}

fn finish(text: String, span: Span) -> Option<HoverResult> {
    (text.len() <= MAXIMUM_HOVER_TEXT_BYTES).then_some(HoverResult { text, span })
}

fn contains_offset(span: Span, offset: usize) -> bool {
    span.start.offset <= offset && offset < span.end.offset
}

#[cfg(test)]
mod tests {
    use yanshu_analysis::analyze_program;
    use yanshu_syntax::load_program_source;

    use super::{MAXIMUM_HOVER_TEXT_BYTES, finish, hover_at};

    const SOURCE: &str = r#"(program
  (name hover-help)
  (version 4)
  (capabilities log)
  (libraries (text 1))
  (schema action (enum "approve" "reject"))
  (data decision (approved (amount integer)))
  (export-types decision)
  (signature choose (fn (integer) integer))
  (def choose
    (fn (amount)
      (cond
        ((> amount 0) (do (log amount) (text/length "AI")))
        (else 0))))
  (signature use (fn (integer) integer))
  (def use (fn (value) (choose value)))
  (export choose use approved))"#;

    fn text_at(source: &str, offset: usize) -> Option<String> {
        let program =
            load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let analysis = analyze_program(&program).ok();
        hover_at(&program, analysis.as_ref(), offset).map(|hover| hover.text)
    }

    fn marker(source: &str, value: &str) -> usize {
        source
            .find(value)
            .unwrap_or_else(|| panic!("marker missing: {value}"))
    }

    #[test]
    fn resolves_forms_primitives_libraries_and_user_functions_exactly() {
        let cond = text_at(SOURCE, marker(SOURCE, "cond\n"))
            .unwrap_or_else(|| panic!("cond hover missing"));
        assert!(cond.contains("kind: short-circuit special form"));
        assert!(cond.contains("since: language v2"));
        assert!(cond.contains("node: expression-v1"));

        let comparison = text_at(SOURCE, marker(SOURCE, "> amount"))
            .unwrap_or_else(|| panic!("comparison hover missing"));
        assert!(comparison.contains("type: fn(Int, Int) -> Bool"));
        assert!(comparison.contains("effects: pure"));

        let log = text_at(SOURCE, marker(SOURCE, "log amount"))
            .unwrap_or_else(|| panic!("log hover missing"));
        assert!(log.contains("effects: log"));
        assert!(log.contains("(capabilities log)"));

        let library = text_at(SOURCE, marker(SOURCE, "text/length"))
            .unwrap_or_else(|| panic!("library hover missing"));
        assert!(library.contains("kind: library operation"));
        assert!(library.contains("library: text@1 (declared)"));
        assert!(library.contains("Unicode scalar"));

        let user_call = SOURCE
            .rfind("choose value")
            .unwrap_or_else(|| panic!("user call marker missing"));
        let user =
            text_at(SOURCE, user_call).unwrap_or_else(|| panic!("user function hover missing"));
        assert!(user.contains("kind: function definition"));
        assert!(user.contains("type: fn(Int) -> Int"));
        assert!(user.contains("effects: log"));
    }

    #[test]
    fn resolves_top_level_type_schema_and_lexical_binding_contexts() {
        let signature = text_at(SOURCE, marker(SOURCE, "signature choose"))
            .unwrap_or_else(|| panic!("signature hover missing"));
        assert!(signature.contains("kind: function type declaration"));
        assert!(signature.contains("since: language v4"));

        let type_form = text_at(SOURCE, marker(SOURCE, "(fn (integer) integer)") + 1)
            .unwrap_or_else(|| panic!("function type hover missing"));
        assert!(type_form.contains("kind: function type"));

        let schema = text_at(SOURCE, marker(SOURCE, "enum \"approve\""))
            .unwrap_or_else(|| panic!("enum hover missing"));
        assert!(schema.contains("kind: schema form"));

        let parameter_use = SOURCE
            .rfind("value)")
            .unwrap_or_else(|| panic!("parameter use marker missing"));
        let parameter =
            text_at(SOURCE, parameter_use).unwrap_or_else(|| panic!("parameter hover missing"));
        assert!(parameter.contains("kind: function parameter"));
        assert!(parameter.contains("scope: lexical"));
    }

    #[test]
    fn lexical_shadowing_wins_and_quoted_data_is_not_described_as_code() {
        let shadowed = "(program (name shadow) (version 4) (signature run (fn (integer) integer)) (def run (fn (value) (let ((log (fn (item) item))) (log value)))) (export run))";
        let call = shadowed
            .rfind("log value")
            .unwrap_or_else(|| panic!("shadowed call missing"));
        let text =
            text_at(shadowed, call).unwrap_or_else(|| panic!("shadowed binding hover missing"));
        assert!(text.contains("kind: let binding"));
        assert!(!text.contains("effects: log"));
        assert!(!text.contains("capabilities log"));

        let quoted = "(program (name quoted) (version 4) (signature run (fn () (list symbol))) (def run (fn () '(cond log))) (export run))";
        let quoted_cond = marker(quoted, "cond log");
        assert_eq!(text_at(quoted, quoted_cond), None);
        let quote = marker(quoted, "'(cond");
        let quote_text = text_at(quoted, quote).unwrap_or_else(|| panic!("quote hover missing"));
        assert!(quote_text.contains("kind: special form"));

        let field_name = "(program (name fields) (version 4) (data sample (made (list integer))) (export-types sample) (signature run (fn (integer) sample)) (def run (fn (value) (made value))) (export run made))";
        let field = marker(field_name, "list integer");
        assert_eq!(text_at(field_name, field), None);
    }

    #[test]
    fn oversized_hover_text_fails_closed() {
        let span = yanshu_diagnostic::Span {
            start: yanshu_diagnostic::Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            end: yanshu_diagnostic::Position {
                offset: 1,
                line: 1,
                column: 2,
            },
        };
        assert!(finish("x".repeat(MAXIMUM_HOVER_TEXT_BYTES), span).is_some());
        assert!(finish("x".repeat(MAXIMUM_HOVER_TEXT_BYTES + 1), span).is_none());
    }
}
