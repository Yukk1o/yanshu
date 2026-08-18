#![forbid(unsafe_code)]

mod effects;
mod infer;
mod review;
mod types;

use std::collections::BTreeMap;

use ail_diagnostic::AilResult;
use ail_syntax::Program;
use serde_json::{Value, json};

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

pub fn analyze_program(program: &Program) -> AilResult<AnalysisReport> {
    let inferred = infer::infer_program(program)?;
    effects::analyze_program_effects(program, inferred)
}

#[cfg(test)]
mod tests {
    use ail_diagnostic::{AilResult, Diagnostic};
    use ail_syntax::load_program_source;
    use serde_json::json;

    use crate::{Type, analyze_program, render_rust_review};

    fn require<T>(result: AilResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }

    fn require_error<T>(result: AilResult<T>) -> Diagnostic {
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
        assert_eq!(review.renderer, "rust-readonly-v1");
        assert!(review.text.contains("READ ONLY"));
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
}
