use std::{fs, path::Path};

use yanshu_syntax::{expression_nodes, load_program_source};

use super::{FormatOptions, format_source};

const COMPACT: &str = r#";; policy header
(program (name formatter-demo) (version 4) (capabilities log) (signature decide (fn (integer) integer)) (def decide ; audit stays visible
(fn (amount) (if (> amount 1000) (do (log amount) amount) 0))) (export decide))
;; policy trailer
"#;

#[test]
fn preserves_comments_semantics_and_stable_expression_ids() {
    let formatted = format_source(COMPACT, FormatOptions::default())
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(formatted.changed);
    assert!(formatted.source.contains(";; policy header"));
    assert!(formatted.source.contains("; audit stays visible"));
    assert!(formatted.source.contains(";; policy trailer"));
    assert!(formatted.source.contains("\n\n  (def decide"));

    let before = load_program_source(COMPACT).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let after =
        load_program_source(&formatted.source).unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let before_ids = expression_nodes(&before)
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    let after_ids = expression_nodes(&after)
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    assert_eq!(before.inspect_json(), after.inspect_json());
    assert_eq!(before_ids, after_ids);
}

#[test]
fn output_is_idempotent_and_uses_one_terminal_newline() {
    let first = format_source(COMPACT, FormatOptions::default())
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    let second = format_source(&first.source, FormatOptions::default())
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(!second.changed);
    assert_eq!(first.source, second.source);
    assert!(second.source.ends_with('\n'));
    assert!(!second.source.ends_with("\n\n"));
}

#[test]
fn rejects_invalid_options_and_bounded_output() {
    let width = format_source(
        COMPACT,
        FormatOptions {
            line_width: 20,
            ..FormatOptions::default()
        },
    )
    .err()
    .unwrap_or_else(|| panic!("invalid line width unexpectedly passed"));
    assert_eq!(width.code, "FORMAT_LINE_WIDTH");

    let output = format_source(
        COMPACT,
        FormatOptions {
            max_output_bytes: 32,
            ..FormatOptions::default()
        },
    )
    .err()
    .unwrap_or_else(|| panic!("small output limit unexpectedly passed"));
    assert_eq!(output.code, "FORMAT_OUTPUT_LIMIT");
}

#[test]
fn formats_the_real_expense_policy_without_semantic_drift() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let path = project_root.join("examples/expenses/service.yan");
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("expense policy fixture failed: {error}"));
    let formatted = format_source(&source, FormatOptions::default())
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(formatted.source.contains("(schema decision-request"));
    assert!(
        formatted
            .source
            .contains("\"hospitalityTotal\" hospitality-total")
    );

    let second = format_source(&formatted.source, FormatOptions::default())
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
    assert!(!second.changed);
}
