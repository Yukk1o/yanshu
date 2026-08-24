#![forbid(unsafe_code)]

mod effects;
mod infer;
mod review;
mod types;

use std::collections::BTreeMap;

use serde_json::{Value, json};
use yanshu_diagnostic::YanshuResult;
use yanshu_syntax::Program;

pub use review::{ReviewDocument, ReviewNode, render_rust_review};
pub use types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionAnalysis {
    pub name: String,
    pub inferred_type: Type,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReport {
    pub definitions: Vec<DefinitionAnalysis>,
    pub exports: BTreeMap<String, Vec<String>>,
    pub capability_closure: Vec<String>,
    pub declared_capabilities: Vec<String>,
    pub unused_capabilities: Vec<String>,
}

impl AnalysisReport {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "definitions": self.definitions.iter().map(DefinitionAnalysis::to_json).collect::<Vec<_>>(),
            "exports": self.exports,
            "capabilityClosure": self.capability_closure,
            "declaredCapabilities": self.declared_capabilities,
            "unusedCapabilities": self.unused_capabilities,
        })
    }
}

impl DefinitionAnalysis {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "type": self.inferred_type.to_json(),
            "capabilities": self.capabilities,
        })
    }
}

pub fn analyze_program(program: &Program) -> YanshuResult<AnalysisReport> {
    let inferred = infer::infer_program(program)?;
    effects::analyze_program_effects(program, inferred)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use yanshu_diagnostic::{Diagnostic, YanshuResult};
    use yanshu_syntax::load_program_source;

    use crate::{Type, analyze_program, render_rust_review};

    fn require<T>(result: YanshuResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn require_error<T>(result: YanshuResult<T>) -> Diagnostic {
        match result {
            Err(diagnostic) => diagnostic,
            Ok(_) => panic!("expected a diagnostic"),
        }
    }

    #[test]
    fn infers_types_and_computes_transitive_capability_closure() {
        let program = require(load_program_source(
            r#"(program
                (name typed-policy)
                (version 4)
                (capabilities kv log clock)
                (data decision
                  (approved (amount integer))
                  (rejected (reason string)))
                (def audit (fn (value) (do (log value) value)))
                (def persist (fn (value) (do (kv-put "latest" value) value)))
                (signature decide (fn (integer) decision))
                (def decide (fn (amount)
                  (if (< amount 0)
                      (audit (rejected "negative"))
                      (persist (audit (approved amount))))))
                (export decide))"#,
        ));
        let report = require(analyze_program(&program));
        assert_eq!(report.capability_closure, ["kv", "log"]);
        assert_eq!(report.unused_capabilities, ["clock"]);
        assert_eq!(report.exports["decide"], ["kv", "log"]);
        assert_eq!(
            report
                .definitions
                .iter()
                .find(|definition| definition.name == "decide")
                .map(|definition| definition.inferred_type.clone()),
            Some(Type::Function {
                parameters: vec![Type::Integer],
                result: Box::new(Type::User("decision".to_owned())),
            })
        );

        let review = render_rust_review(&program, &report);
        assert!(!review.editable);
        assert_eq!(review.renderer, "rust-readonly-v3");
        assert!(review.text.contains("READ ONLY"));
        assert!(review.text.contains("Int = arbitrary-precision integer"));
        assert!(review.text.contains("log!(value)"));
        assert!(review.text.contains("audit!"));
        assert!(review.text.contains("if truthy((amount < 0))"));
        assert!(review.text.contains("enum Decision"));
        assert!(review.text.contains("fn decide(amount: Int) -> Decision"));
        assert!(
            review
                .nodes
                .iter()
                .any(|node| node.id == "definition:decide")
        );
    }

    #[test]
    fn rejects_type_mismatch_and_undeclared_effects() {
        let mismatch = require(load_program_source(
            r#"(program
                (name mismatch)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value) (string-append "value:" value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&mismatch));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");

        let undeclared = require(load_program_source(
            r#"(program
                (name undeclared)
                (version 4)
                (signature run (fn (string) string))
                (def run (fn (value) (do (log value) value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&undeclared));
        assert_eq!(diagnostic.code, "EFFECT_CAPABILITY_NOT_DECLARED");
        assert_eq!(diagnostic.details["missing"], json!(["log"]));
    }

    #[test]
    fn follows_known_callbacks_and_rejects_unresolved_export_callbacks() {
        let known = require(load_program_source(
            r#"(program
                (name callbacks)
                (version 4)
                (capabilities log)
                (def emit (fn (value) (do (log value) value)))
                (signature run (fn ((list integer)) (list integer)))
                (def run (fn (values) (list-map emit values)))
                (export run))"#,
        ));
        let report = require(analyze_program(&known));
        assert_eq!(report.capability_closure, ["log"]);

        let unknown = require(load_program_source(
            r#"(program
                (name callback-api)
                (version 4)
                (signature run (fn ((fn (integer) integer) integer) integer))
                (def run (fn (callback value) (callback value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&unknown));
        assert_eq!(diagnostic.code, "EFFECT_UNRESOLVED_PARAMETER");
    }

    #[test]
    fn lexical_bindings_take_precedence_over_primitive_names() {
        let valid = require(load_program_source(
            r#"(program
                (name lexical-callback)
                (version 4)
                (def stringify (fn (value) (number->string value)))
                (def call (fn (length value) (length value)))
                (signature run (fn (integer) string))
                (def run (fn (value) (call stringify value)))
                (export run))"#,
        ));
        let report = require(analyze_program(&valid));
        assert!(report.capability_closure.is_empty());

        let invalid = require(load_program_source(
            r#"(program
                (name lexical-not-callable)
                (version 4)
                (signature run (fn (integer) map))
                (def run (fn (map) (map)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&invalid));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");
    }

    #[test]
    fn effect_analysis_never_resolves_a_pattern_binding_as_a_global_definition() {
        let program = require(load_program_source(
            r#"(program
                (name pattern-callback)
                (version 4)
                (capabilities log)
                (data holder (boxed (callback (fn (integer) integer))))
                (def callback (fn (value) value))
                (def emit (fn (value) (do (log value) value)))
                (signature run (fn (integer) integer))
                (def run (fn (value)
                  (match (boxed emit)
                    ((boxed callback) (callback value))
                    (_ value))))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&program));
        assert_eq!(diagnostic.code, "EFFECT_UNRESOLVED_PARAMETER");
    }

    #[test]
    fn review_effect_markers_respect_lexical_shadowing() {
        let program = require(load_program_source(
            r#"(program
                (name review-shadowing)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value)
                  (do
                    (let ((log (fn (item) item))) (log value))
                    (let ((map (fn (item) item))) (map value)))))
                (export run))"#,
        ));
        let report = require(analyze_program(&program));
        let review = render_rust_review(&program, &report);
        assert_eq!(review.renderer, "rust-readonly-v3");
        assert!(review.text.contains("log(value)"));
        assert!(review.text.contains("map(value)"));
        assert!(!review.text.contains("log!(value)"));
        assert!(!review.text.contains("map {"));
    }

    #[test]
    fn effect_analysis_preserves_callable_closure_captures() {
        let program = require(load_program_source(
            r#"(program
                (name captured-effects)
                (version 4)
                (capabilities log)
                (def callback (fn (value) value))
                (signature run (fn (integer) integer))
                (def run (fn (value)
                  (let ((callback (fn (item) (do (log item) item)))
                        (wrapped (fn (item) (callback item))))
                    (wrapped value))))
                (export run))"#,
        ));
        let report = require(analyze_program(&program));
        assert_eq!(report.capability_closure, ["log"]);
        assert_eq!(report.exports["run"], ["log"]);
    }

    #[test]
    fn kv_contracts_check_arity_and_match_runtime_return_types() {
        let valid = require(load_program_source(
            r#"(program
                (name kv-types)
                (version 4)
                (capabilities kv)
                (signature run (fn (string) boolean))
                (def run (fn (key) (kv-delete key)))
                (export run))"#,
        ));
        let report = require(analyze_program(&valid));
        assert_eq!(report.capability_closure, ["kv"]);

        for call in ["(kv-put)", "(kv-delete)", "(kv-list)"] {
            let source = format!(
                "(program (name bad-kv) (version 4) (capabilities kv) \
                 (signature run (fn () any)) (def run (fn () {call})) (export run))"
            );
            let program = require(load_program_source(&source));
            let diagnostic = require_error(analyze_program(&program));
            assert_eq!(diagnostic.code, "TYPE_ARITY", "call: {call}");
        }
    }

    #[test]
    fn versioned_library_contracts_drive_calls_and_first_class_function_types() {
        let program = require(load_program_source(
            r#"(program
                (name text-v2-types)
                (version 4)
                (libraries (text 2))
                (signature split (fn (string) (list string)))
                (def split (fn (value) (text/split (text/trim value) ",")))
                (signature lower-all (fn ((list string)) (list string)))
                (def lower-all (fn (values) (list-map text/lowercase values)))
                (export split lower-all))"#,
        ));
        let report = require(analyze_program(&program));
        assert!(report.capability_closure.is_empty());
        assert_eq!(
            report
                .definitions
                .iter()
                .find(|definition| definition.name == "split")
                .map(|definition| definition.inferred_type.clone()),
            Some(Type::Function {
                parameters: vec![Type::String],
                result: Box::new(Type::List(Box::new(Type::String))),
            })
        );

        let wrong_list = require(load_program_source(
            r#"(program
                (name text-v2-wrong-list)
                (version 4)
                (libraries (text 2))
                (signature run (fn () string))
                (def run (fn () (text/join (list 1 2) ",")))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&wrong_list));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");

        let v1 = require(load_program_source(
            r#"(program
                (name text-v1-boundary)
                (version 4)
                (libraries (text 1))
                (signature run (fn (string) string))
                (def run (fn (value) (text/trim value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&v1));
        assert_eq!(diagnostic.code, "TYPE_UNBOUND_NAME");

        let math = require(load_program_source(
            r#"(program
                (name math-v1-types)
                (version 4)
                (libraries (math 1))
                (signature magnitudes (fn ((list integer)) (list integer)))
                (def magnitudes (fn (values) (list-map math/abs values)))
                (signature bounded (fn (integer integer integer) integer))
                (def bounded (fn (value minimum maximum)
                  (math/clamp value minimum maximum)))
                (export magnitudes bounded))"#,
        ));
        let report = require(analyze_program(&math));
        assert!(report.capability_closure.is_empty());

        let wrong_math = require(load_program_source(
            r#"(program
                (name math-v1-wrong-type)
                (version 4)
                (libraries (math 1))
                (signature run (fn (string) integer))
                (def run (fn (value) (math/abs value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&wrong_math));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");

        let digest = require(load_program_source(
            r#"(program
                (name digest-v1-types)
                (version 4)
                (libraries (digest 1))
                (signature hashes (fn ((list string)) (list string)))
                (def hashes (fn (values) (list-map digest/sha256-text values)))
                (signature hash-one (fn (string) string))
                (def hash-one (fn (value) (digest/sha512-text value)))
                (export hashes hash-one))"#,
        ));
        let report = require(analyze_program(&digest));
        assert!(report.capability_closure.is_empty());

        let wrong_digest = require(load_program_source(
            r#"(program
                (name digest-v1-wrong-type)
                (version 4)
                (libraries (digest 1))
                (signature run (fn (integer) string))
                (def run (fn (value) (digest/sha256-text value)))
                (export run))"#,
        ));
        let diagnostic = require_error(analyze_program(&wrong_digest));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");
    }
}
