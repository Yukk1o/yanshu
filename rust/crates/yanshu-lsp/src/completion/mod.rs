mod context;

use std::collections::BTreeMap;

use serde_json::{Value, json};
use yanshu_analysis::{AnalysisReport, Type};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_library::{FuelModel, trusted_contract};
use yanshu_syntax::{
    Datum, DatumKind, ExpressionKind, Program, ReaderLimits, SymbolBinding, SymbolBindingKind,
    read_source, symbol_index,
};

use crate::hover::catalog::{FormContext, form_entries, primitive_entries};

use context::{CompletionSite, SiteKind, completion_site};

pub(crate) const MAXIMUM_COMPLETION_ITEMS: usize = 128;
pub(crate) const MAXIMUM_COMPLETION_TEXT_BYTES: usize = 256 * 1024;
const MAXIMUM_COMPLETION_JSON_OVERHEAD_BYTES: usize = 2 * 1024;

// Every candidate string byte can expand to a six-byte JSON escape. Account
// separately for the fixed fields, range, and punctuation of every item.
const _: () = assert!(
    MAXIMUM_COMPLETION_TEXT_BYTES * 6
        + MAXIMUM_COMPLETION_ITEMS * MAXIMUM_COMPLETION_JSON_OVERHEAD_BYTES
        < crate::protocol::MAXIMUM_LSP_MESSAGE_BYTES
);

const KIND_FUNCTION: u8 = 3;
const KIND_VARIABLE: u8 = 6;
const KIND_CLASS: u8 = 7;
const KIND_VALUE: u8 = 12;
const KIND_KEYWORD: u8 = 14;

const BUILTIN_TYPES: &[(&str, &str)] = &[
    ("any", "Any bounded portable guest value."),
    ("boolean", "Boolean value."),
    ("integer", "Arbitrary-precision integer."),
    ("map", "Portable string-keyed map."),
    ("nil", "The Nil value."),
    ("string", "Unicode string."),
    ("symbol", "Guest symbol value."),
];

const SCHEMA_ATOMS: &[(&str, &str)] = &[
    ("any", "Schema accepting any bounded portable guest value."),
    ("boolean", "Schema accepting only #t or #f."),
    ("integer", "Unbounded arbitrary-precision integer schema."),
    ("string", "Unbounded Unicode string schema."),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionCandidate {
    label: String,
    kind: u8,
    detail: String,
    documentation: String,
    sort_group: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionResult {
    candidates: Vec<CompletionCandidate>,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
}

impl CompletionResult {
    pub(crate) fn into_lsp(self, range: Value) -> Value {
        let items = self
            .candidates
            .into_iter()
            .map(|candidate| {
                let sort_text = format!("{:02}:{}", candidate.sort_group, candidate.label);
                json!({
                    "label": candidate.label,
                    "kind": candidate.kind,
                    "detail": candidate.detail,
                    "documentation": {
                        "kind": "plaintext",
                        "value": candidate.documentation,
                    },
                    "sortText": sort_text,
                    "filterText": candidate.label,
                    "textEdit": {
                        "range": range,
                        "newText": candidate.label,
                    },
                })
            })
            .collect::<Vec<_>>();
        json!({
            "isIncomplete": false,
            "items": items,
        })
    }
}

pub(crate) fn completion_at(
    source: &str,
    program: Option<&Program>,
    analysis: Option<&AnalysisReport>,
    offset: usize,
) -> YanshuResult<Option<CompletionResult>> {
    let root = match read_source(source, ReaderLimits::default()) {
        Ok(root) => root,
        Err(_) => return Ok(None),
    };
    let Some(site) = completion_site(&root, source, offset) else {
        return Ok(None);
    };
    let version = program
        .and_then(program_version)
        .or_else(|| reader_version(&root))
        .unwrap_or(1);
    let mut candidates = BTreeMap::new();

    match site.kind {
        SiteKind::TopLevel { root } => {
            add_forms(
                &mut candidates,
                &site,
                FormContext::TopLevel,
                version,
                Some(root),
            );
        }
        SiteKind::Type { head } => {
            if head {
                add_forms(&mut candidates, &site, FormContext::Type, version, None);
            }
            if version >= 4 {
                for (name, documentation) in BUILTIN_TYPES {
                    insert(
                        &mut candidates,
                        &site.prefix,
                        CompletionCandidate {
                            label: (*name).to_owned(),
                            kind: KIND_CLASS,
                            detail: "built-in type".to_owned(),
                            documentation: (*documentation).to_owned(),
                            sort_group: 1,
                        },
                    );
                }
                if let Some(program) = program {
                    for data_type in &program.data_types {
                        insert(
                            &mut candidates,
                            &site.prefix,
                            CompletionCandidate {
                                label: data_type.name.clone(),
                                kind: KIND_CLASS,
                                detail: "user-defined closed data type".to_owned(),
                                documentation:
                                    "A data type declared by this program and valid in signatures."
                                        .to_owned(),
                                sort_group: 0,
                            },
                        );
                    }
                }
            }
        }
        SiteKind::Schema { head } => {
            if head {
                add_forms(&mut candidates, &site, FormContext::Schema, version, None);
            }
            for (name, documentation) in SCHEMA_ATOMS {
                insert(
                    &mut candidates,
                    &site.prefix,
                    CompletionCandidate {
                        label: (*name).to_owned(),
                        kind: KIND_VALUE,
                        detail: "schema atom".to_owned(),
                        documentation: (*documentation).to_owned(),
                        sort_group: 1,
                    },
                );
            }
        }
        SiteKind::Expression { head } => {
            if head {
                add_forms(
                    &mut candidates,
                    &site,
                    FormContext::Expression,
                    version,
                    None,
                );
            }
            if let Some(program) = program {
                add_program_values(&mut candidates, &site, program, analysis, offset, version);
            }
        }
        SiteKind::Other => return Ok(None),
    }

    finish(candidates, site)
}

fn add_forms(
    candidates: &mut BTreeMap<String, CompletionCandidate>,
    site: &CompletionSite,
    context: FormContext,
    version: u8,
    root_only: Option<bool>,
) {
    for entry in form_entries(context) {
        if entry.minimum_version > version {
            continue;
        }
        if let Some(root) = root_only
            && ((root && entry.name != "program") || (!root && entry.name == "program"))
        {
            continue;
        }
        insert(
            candidates,
            &site.prefix,
            CompletionCandidate {
                label: entry.name.to_owned(),
                kind: KIND_KEYWORD,
                detail: format!("{} · since v{}", entry.kind, entry.minimum_version),
                documentation: format!("syntax: {}\n{}", entry.syntax, entry.summary),
                sort_group: 0,
            },
        );
    }
}

fn add_program_values(
    candidates: &mut BTreeMap<String, CompletionCandidate>,
    site: &CompletionSite,
    program: &Program,
    analysis: Option<&AnalysisReport>,
    offset: usize,
    version: u8,
) {
    if let Ok(index) = symbol_index(program) {
        for binding in index.visible_bindings_at(offset) {
            insert(
                candidates,
                &site.prefix,
                binding_candidate(program, analysis, binding),
            );
        }
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
                        .map_or(Type::Any, Type::from_expression)
                })
                .collect();
            let value_type = Type::Function {
                parameters,
                result: Box::new(Type::User(data_type.name.clone())),
            };
            insert(
                candidates,
                &site.prefix,
                CompletionCandidate {
                    label: variant.name.clone(),
                    kind: KIND_FUNCTION,
                    detail: format!("data constructor · {}", value_type.display()),
                    documentation: format!(
                        "Constructs one closed {} variant with fields in declaration order.",
                        data_type.name
                    ),
                    sort_group: 2,
                },
            );
        }
    }

    for schema in &program.schemas {
        insert(
            candidates,
            &site.prefix,
            CompletionCandidate {
                label: schema.name.clone(),
                kind: KIND_VALUE,
                detail: "schema value".to_owned(),
                documentation:
                    "A bounded schema value declared by this program for validate operations."
                        .to_owned(),
                sort_group: 2,
            },
        );
    }

    for primitive in primitive_entries() {
        if primitive.minimum_version > version
            || (primitive.effects != "pure"
                && !program
                    .capabilities
                    .iter()
                    .any(|capability| capability == primitive.effects))
        {
            continue;
        }
        let mut documentation = primitive.summary.to_owned();
        if let Some(requirement) = primitive.requirement {
            documentation.push_str("\nrequires: ");
            documentation.push_str(requirement);
        }
        if let Some(metering) = primitive.metering {
            documentation.push_str("\nfuel: ");
            documentation.push_str(metering);
        }
        insert(
            candidates,
            &site.prefix,
            CompletionCandidate {
                label: primitive.name.to_owned(),
                kind: KIND_FUNCTION,
                detail: format!(
                    "core primitive · {} · effects: {}",
                    primitive.signature, primitive.effects
                ),
                documentation,
                sort_group: 3,
            },
        );
    }

    for requirement in &program.libraries {
        let Some(contract) = trusted_contract(&requirement.name, requirement.version) else {
            continue;
        };
        for operation in contract.operations {
            let label = format!("{}/{}", contract.name, operation.name);
            let parameters = operation
                .parameters
                .iter()
                .map(|parameter| parameter.display())
                .collect::<Vec<_>>()
                .join(", ");
            insert(
                candidates,
                &site.prefix,
                CompletionCandidate {
                    label,
                    kind: KIND_FUNCTION,
                    detail: format!(
                        "library operation · fn({parameters}) -> {} · {}@{}",
                        operation.result.display(),
                        contract.name,
                        contract.version
                    ),
                    documentation: format!(
                        "Trusted Library Backend operation.\neffects: pure\nfuel: {}",
                        fuel_description(operation.fuel)
                    ),
                    sort_group: 4,
                },
            );
        }
    }
}

fn binding_candidate(
    program: &Program,
    analysis: Option<&AnalysisReport>,
    binding: &SymbolBinding,
) -> CompletionCandidate {
    match binding.kind {
        SymbolBindingKind::Definition => {
            let is_function = program
                .definitions
                .iter()
                .find(|definition| definition.name == binding.name)
                .is_some_and(|definition| {
                    matches!(definition.expression.kind, ExpressionKind::Function { .. })
                });
            let mut detail = if is_function {
                "function definition".to_owned()
            } else {
                "global definition".to_owned()
            };
            if let Some(value_type) = definition_type(program, analysis, &binding.name) {
                detail.push_str(" · ");
                detail.push_str(&value_type.display());
            }
            if let Some(definition) = analysis.and_then(|report| {
                report
                    .definitions
                    .iter()
                    .find(|definition| definition.name == binding.name)
            }) {
                detail.push_str(" · effects: ");
                if definition.capabilities.is_empty() {
                    detail.push_str("pure");
                } else {
                    detail.push_str(&definition.capabilities.join(", "));
                }
            }
            CompletionCandidate {
                label: binding.name.clone(),
                kind: if is_function {
                    KIND_FUNCTION
                } else {
                    KIND_VARIABLE
                },
                detail,
                documentation:
                    "A guest definition visible from this expression in the current program."
                        .to_owned(),
                sort_group: 1,
            }
        }
        SymbolBindingKind::Parameter | SymbolBindingKind::Let | SymbolBindingKind::Pattern => {
            let kind = match binding.kind {
                SymbolBindingKind::Parameter => "function parameter",
                SymbolBindingKind::Let => "let binding",
                SymbolBindingKind::Pattern => "pattern binding",
                SymbolBindingKind::Definition => unreachable!(),
            };
            CompletionCandidate {
                label: binding.name.clone(),
                kind: KIND_VARIABLE,
                detail: format!("{kind} · lexical scope"),
                documentation: "A lexical guest binding visible at exactly this source position."
                    .to_owned(),
                sort_group: 0,
            }
        }
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

fn fuel_description(model: FuelModel) -> String {
    match model {
        FuelModel::Fixed(value) => format!("{value} per call"),
        FuelModel::TextCharacters { base, block_size } => {
            format!("{base} + ceil(total Unicode scalar count / {block_size})")
        }
        FuelModel::TextReplace { base, block_size } => {
            format!("{base} + ceil(text/replacement work and output bytes / {block_size})")
        }
        FuelModel::TextCase {
            base, block_size, ..
        } => format!("{base} + ceil(Unicode case work and output bytes / {block_size})"),
        FuelModel::TextSplit { base, block_size } => {
            format!("{base} + ceil(split scan, output bytes, and segments / {block_size})")
        }
        FuelModel::TextJoin { base, block_size } => {
            format!("{base} + ceil(join input, output bytes, and item count / {block_size})")
        }
        FuelModel::TextSubstring { base, block_size } => {
            format!("{base} + ceil(scalar scan and output bytes / {block_size})")
        }
        FuelModel::IntegerLinear { base, block_size } => {
            format!("{base} + total ceil(integer magnitude bits / {block_size})")
        }
        FuelModel::IntegerClamp { base, block_size } => {
            format!("{base} + ordered clamp bounds and total integer blocks of {block_size} bits")
        }
        FuelModel::IntegerGcd { base, block_size } => {
            format!("{base} + product of integer magnitude blocks of {block_size} bits")
        }
        FuelModel::Utf8Bytes { base, block_size } => {
            format!("{base} + ceil(total UTF-8 input bytes / {block_size})")
        }
        FuelModel::JsonParse { base, block_size } => {
            format!("{base} + ceil(JSON UTF-8 input bytes / {block_size})")
        }
        FuelModel::JsonStringify { base, block_size } => {
            format!("{base} + ceil(JSON value traversal and text bytes / {block_size})")
        }
        FuelModel::DecimalParse { base, block_size } => {
            format!("{base} + ceil(decimal input, scale, and padding work / {block_size})")
        }
        FuelModel::DecimalFormat { base, block_size } => {
            format!("{base} + ceil(integer magnitude and decimal output work / {block_size})")
        }
        FuelModel::DecimalRescale { base, block_size } => {
            format!("{base} + ceil(integer magnitude and scale delta work / {block_size})")
        }
        FuelModel::ListStructural {
            base, block_size, ..
        } => format!(
            "{base} + ceil(portable input traversal and selected output clone work / {block_size})"
        ),
    }
}

fn insert(
    candidates: &mut BTreeMap<String, CompletionCandidate>,
    prefix: &str,
    candidate: CompletionCandidate,
) {
    if candidate.label.starts_with(prefix) {
        candidates
            .entry(candidate.label.clone())
            .or_insert(candidate);
    }
}

fn finish(
    candidates: BTreeMap<String, CompletionCandidate>,
    site: CompletionSite,
) -> YanshuResult<Option<CompletionResult>> {
    if candidates.len() > MAXIMUM_COMPLETION_ITEMS {
        return Err(Diagnostic::new(
            "LSP_COMPLETION_LIMIT",
            "LSP completion result exceeds the configured item limit",
            json!({
                "actual": candidates.len(),
                "maximum": MAXIMUM_COMPLETION_ITEMS,
            }),
        ));
    }
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        (left.sort_group, left.label.as_str()).cmp(&(right.sort_group, right.label.as_str()))
    });
    let text_bytes = candidates.iter().fold(0_usize, |total, candidate| {
        let sort_bytes = 3_usize.saturating_add(candidate.label.len());
        total.saturating_add(
            candidate
                .label
                .len()
                .saturating_mul(3)
                .saturating_add(candidate.detail.len())
                .saturating_add(candidate.documentation.len())
                .saturating_add(sort_bytes),
        )
    });
    if text_bytes > MAXIMUM_COMPLETION_TEXT_BYTES {
        return Err(Diagnostic::new(
            "LSP_COMPLETION_LIMIT",
            "LSP completion result exceeds the configured text byte limit",
            json!({
                "actual": text_bytes,
                "maximum": MAXIMUM_COMPLETION_TEXT_BYTES,
            }),
        ));
    }
    Ok(Some(CompletionResult {
        candidates,
        replace_start: site.replace_start,
        replace_end: site.replace_end,
    }))
}

fn program_version(program: &Program) -> Option<u8> {
    program.version.to_string().parse().ok()
}

fn reader_version(root: &Datum) -> Option<u8> {
    let program = root.list()?;
    if program.first().and_then(Datum::symbol) != Some("program") {
        return None;
    }
    program[1..].iter().find_map(|datum| {
        let form = datum.list()?;
        if form.first().and_then(Datum::symbol) != Some("version") {
            return None;
        }
        let DatumKind::Integer(version) = &form.get(1)?.kind else {
            return None;
        };
        version.to_string().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use yanshu_analysis::analyze_program;
    use yanshu_syntax::load_program_source;

    use super::context::{CompletionSite, SiteKind};
    use super::{
        CompletionCandidate, CompletionResult, KIND_VALUE, MAXIMUM_COMPLETION_ITEMS,
        MAXIMUM_COMPLETION_TEXT_BYTES, completion_at, finish,
    };

    const SOURCE: &str = r#"(program
  (name completion)
  (version 4)
  (capabilities log)
  (libraries (text 1))
  (schema request string)
  (data decision (approved (amount integer)))
  (signature target (fn (integer) integer))
  (def target (fn (value) value))
  (signature use (fn (integer) integer))
  (def use
    (fn (value)
      (let ((target (fn (item) item))
            (later value))
        (target value))))
  (export target use approved))"#;

    fn result_at(source: &str, marker: &str, delta: usize) -> CompletionResult {
        let program =
            load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let analysis = analyze_program(&program).ok();
        let offset = source
            .rfind(marker)
            .unwrap_or_else(|| panic!("marker missing: {marker}"))
            + delta;
        completion_at(source, Some(&program), analysis.as_ref(), offset)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("completion missing"))
    }

    fn labels(result: &CompletionResult) -> Vec<&str> {
        result
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect()
    }

    #[test]
    fn lexical_scope_shadows_globals_and_candidates_match_declared_authority() {
        let result = result_at(SOURCE, "target value", 0);
        let labels = labels(&result);
        assert!(labels.contains(&"target"));
        assert!(labels.contains(&"value"));
        assert!(labels.contains(&"later"));
        assert!(labels.contains(&"log"));
        assert!(labels.contains(&"text/length"));
        assert!(labels.contains(&"approved"));
        assert!(labels.contains(&"request"));
        assert!(!labels.contains(&"now-ms"));
        assert_eq!(
            result
                .candidates
                .iter()
                .find(|candidate| candidate.label == "target")
                .map(|candidate| candidate.detail.as_str()),
            Some("let binding · lexical scope")
        );
        assert_eq!(&SOURCE[result.replace_start..result.replace_end], "target");
    }

    #[test]
    fn completion_uses_the_declared_text_contract_version() {
        let source = r#"(program
          (name completion-text-v2)
          (version 4)
          (libraries (text 2))
          (signature run (fn (string) string))
          (def run (fn (value) (text/lo value)))
          (export run))"#;
        let result = result_at(source, "text/lo", "text/lo".len());
        assert_eq!(labels(&result), ["text/lowercase"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("text@2 completion missing"));
        assert!(candidate.detail.contains("fn(String) -> String"));
        assert!(candidate.detail.contains("text@2"));
        assert!(candidate.documentation.contains("Unicode case work"));
    }

    #[test]
    fn completion_uses_the_declared_math_contract() {
        let source = r#"(program
          (name completion-math-v1)
          (version 4)
          (libraries (math 1))
          (signature run (fn (integer integer) integer))
          (def run (fn (left right) (math/g left right)))
          (export run))"#;
        let result = result_at(source, "math/g", "math/g".len());
        assert_eq!(labels(&result), ["math/gcd"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("math@1 completion missing"));
        assert!(candidate.detail.contains("fn(Int, Int) -> Int"));
        assert!(candidate.detail.contains("math@1"));
        assert!(candidate.documentation.contains("magnitude blocks"));
    }

    #[test]
    fn completion_uses_the_declared_digest_contract() {
        let source = r#"(program
          (name completion-digest-v1)
          (version 4)
          (libraries (digest 1))
          (signature run (fn (string) string))
          (def run (fn (value) (digest/sha256-t value)))
          (export run))"#;
        let result = result_at(source, "digest/sha256-t", "digest/sha256-t".len());
        assert_eq!(labels(&result), ["digest/sha256-text"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("digest@1 completion missing"));
        assert!(candidate.detail.contains("fn(String) -> String"));
        assert!(candidate.detail.contains("digest@1"));
        assert!(candidate.documentation.contains("UTF-8 input bytes"));
    }

    #[test]
    fn completion_uses_the_declared_json_contract() {
        let source = r#"(program
          (name completion-json-v1)
          (version 4)
          (libraries (json 1))
          (signature run (fn (string) (result any any)))
          (def run (fn (value) (json/pa value)))
          (export run))"#;
        let result = result_at(source, "json/pa", "json/pa".len());
        assert_eq!(labels(&result), ["json/parse"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("json@1 completion missing"));
        assert!(candidate.detail.contains("fn(String) -> Result"));
        assert!(candidate.detail.contains("json@1"));
        assert!(candidate.documentation.contains("JSON UTF-8 input bytes"));
    }

    #[test]
    fn completion_uses_the_declared_decimal_contract() {
        let source = r#"(program
          (name completion-decimal-v1)
          (version 4)
          (libraries (decimal 1))
          (signature run (fn (integer) (result any any)))
          (def run (fn (value) (decimal/res value 3 2 "half-even")))
          (export run))"#;
        let result = result_at(source, "decimal/res", "decimal/res".len());
        assert_eq!(labels(&result), ["decimal/rescale"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("decimal@1 completion missing"));
        assert!(
            candidate
                .detail
                .contains("fn(Int, Int, Int, String) -> Result")
        );
        assert!(candidate.detail.contains("decimal@1"));
        assert!(candidate.documentation.contains("scale delta work"));
    }

    #[test]
    fn completion_uses_the_declared_list_contract() {
        let source = r#"(program
          (name completion-list-v1)
          (version 4)
          (libraries (list 1))
          (signature run (fn ((list integer) integer integer) (result any any)))
          (def run (fn (values start end) (list/sli values start end)))
          (export run))"#;
        let result = result_at(source, "list/sli", "list/sli".len());
        assert_eq!(labels(&result), ["list/slice"]);
        let candidate = result
            .candidates
            .first()
            .unwrap_or_else(|| panic!("list@1 completion missing"));
        assert!(candidate.detail.contains("fn(List, Int, Int) -> Result"));
        assert!(candidate.detail.contains("list@1"));
        assert!(
            candidate
                .documentation
                .contains("selected output clone work")
        );
    }

    #[test]
    fn version_filter_and_reader_only_top_level_completion_fail_closed() {
        let old = "(program (name old) (version 1) (def run (fn () (number))) (export run))";
        let old_result = result_at(old, "number", 6);
        assert!(!labels(&old_result).contains(&"number->string"));

        let partial = "(program (name partial) (version 4) (sig))";
        let offset = partial
            .find("sig")
            .unwrap_or_else(|| panic!("partial marker missing"))
            + 3;
        let result = completion_at(partial, None, None, offset)
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"))
            .unwrap_or_else(|| panic!("reader-only completion missing"));
        assert_eq!(labels(&result), ["signature"]);
        assert_eq!(&partial[result.replace_start..result.replace_end], "sig");
    }

    #[test]
    fn quoted_comment_and_string_data_have_no_completion() {
        let source = "(program (name data) (version 4) (signature run (fn () symbol)) (def run (fn () '(log cond))) (export run))";
        let program =
            load_program_source(source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let quoted = source
            .find("log cond")
            .unwrap_or_else(|| panic!("quoted marker missing"));
        assert_eq!(
            completion_at(source, Some(&program), None, quoted)
                .unwrap_or_else(|diagnostic| panic!("{diagnostic}")),
            None
        );
    }

    #[test]
    fn completion_is_deterministic_and_serializes_exact_edits() {
        let first = result_at(SOURCE, "target value", 0);
        let second = result_at(SOURCE, "target value", 0);
        assert_eq!(first, second);
        let rendered = first.into_lsp(json!({
            "start": { "line": 1, "character": 2 },
            "end": { "line": 1, "character": 8 },
        }));
        assert_eq!(rendered["isIncomplete"], false);
        assert!(rendered["items"].as_array().is_some_and(|items| {
            items.iter().all(|item| {
                item["textEdit"]["range"]["start"]["character"] == 2
                    && item["textEdit"]["range"]["end"]["character"] == 8
            })
        }));
    }

    #[test]
    fn completion_limits_fail_closed_with_a_stable_diagnostic() {
        let site = CompletionSite {
            prefix: String::new(),
            replace_start: 0,
            replace_end: 0,
            kind: SiteKind::Expression { head: true },
        };
        let mut too_many = BTreeMap::new();
        for index in 0..=MAXIMUM_COMPLETION_ITEMS {
            let label = format!("item-{index:03}");
            too_many.insert(
                label.clone(),
                CompletionCandidate {
                    label,
                    kind: KIND_VALUE,
                    detail: String::new(),
                    documentation: String::new(),
                    sort_group: 0,
                },
            );
        }
        let diagnostic = match finish(too_many, site.clone()) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("item limit accepted"),
        };
        assert_eq!(diagnostic.code, "LSP_COMPLETION_LIMIT");

        let label = "large".to_owned();
        let mut too_large = BTreeMap::new();
        too_large.insert(
            label.clone(),
            CompletionCandidate {
                label,
                kind: KIND_VALUE,
                detail: String::new(),
                documentation: "x".repeat(MAXIMUM_COMPLETION_TEXT_BYTES),
                sort_group: 0,
            },
        );
        let diagnostic = match finish(too_large, site) {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("text limit accepted"),
        };
        assert_eq!(diagnostic.code, "LSP_COMPLETION_LIMIT");
    }
}
