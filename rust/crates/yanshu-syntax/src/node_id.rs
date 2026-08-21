use yanshu_diagnostic::Span;

use crate::{Expression, ExpressionKind, Program};

/// A deterministic semantic path for one expression in a parsed program.
///
/// IDs do not contain source offsets, so whitespace-only formatting does not
/// change them. They are review and tooling identities, not content hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionNode {
    pub id: String,
    pub span: Span,
}

#[must_use]
pub fn expression_nodes(program: &Program) -> Vec<ExpressionNode> {
    let mut nodes = Vec::new();
    for definition in &program.definitions {
        let root = format!(
            "expression-v1/definition/{}",
            escape_segment(&definition.name)
        );
        collect_expression(&definition.expression, &root, &mut nodes);
    }
    nodes
}

fn collect_expression(expression: &Expression, id: &str, nodes: &mut Vec<ExpressionNode>) {
    nodes.push(ExpressionNode {
        id: id.to_owned(),
        span: expression.span,
    });
    match &expression.kind {
        ExpressionKind::Literal(_) | ExpressionKind::Variable(_) | ExpressionKind::Quote(_) => {}
        ExpressionKind::If {
            condition,
            consequent,
            alternative,
        } => {
            collect_child(condition, id, "if/condition", nodes);
            collect_child(consequent, id, "if/consequent", nodes);
            collect_child(alternative, id, "if/alternative", nodes);
        }
        ExpressionKind::And(expressions) => collect_indexed(expressions, id, "and", nodes),
        ExpressionKind::Or(expressions) => collect_indexed(expressions, id, "or", nodes),
        ExpressionKind::Cond {
            clauses,
            alternative,
        } => {
            for (index, clause) in clauses.iter().enumerate() {
                collect_child(
                    &clause.condition,
                    id,
                    &format!("cond/clause/{index}/condition"),
                    nodes,
                );
                collect_child(
                    &clause.expression,
                    id,
                    &format!("cond/clause/{index}/expression"),
                    nodes,
                );
            }
            collect_child(alternative, id, "cond/alternative", nodes);
        }
        ExpressionKind::Match { value, arms } => {
            collect_child(value, id, "match/value", nodes);
            for (index, arm) in arms.iter().enumerate() {
                collect_child(
                    &arm.expression,
                    id,
                    &format!("match/arm/{index}/expression"),
                    nodes,
                );
            }
        }
        ExpressionKind::Let { bindings, body } => {
            for (index, binding) in bindings.iter().enumerate() {
                collect_child(
                    &binding.expression,
                    id,
                    &format!("let/binding/{index}/{}", escape_segment(&binding.name)),
                    nodes,
                );
            }
            collect_child(body, id, "let/body", nodes);
        }
        ExpressionKind::Function { body, .. } => collect_child(body, id, "function/body", nodes),
        ExpressionKind::Do(expressions) => collect_indexed(expressions, id, "do", nodes),
        ExpressionKind::Call { callee, arguments } => {
            collect_child(callee, id, "call/callee", nodes);
            collect_indexed(arguments, id, "call/argument", nodes);
        }
    }
}

fn collect_child(
    expression: &Expression,
    parent: &str,
    suffix: &str,
    nodes: &mut Vec<ExpressionNode>,
) {
    collect_expression(expression, &format!("{parent}/{suffix}"), nodes);
}

fn collect_indexed(
    expressions: &[Expression],
    parent: &str,
    role: &str,
    nodes: &mut Vec<ExpressionNode>,
) {
    for (index, expression) in expressions.iter().enumerate() {
        collect_child(expression, parent, &format!("{role}/{index}"), nodes);
    }
}

fn escape_segment(value: &str) -> String {
    let mut escaped = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            escaped.push(char::from(byte));
        } else {
            escaped.push('%');
            escaped.push(hex_digit(byte >> 4));
            escaped.push(hex_digit(byte & 0x0f));
        }
    }
    escaped
}

fn hex_digit(value: u8) -> char {
    char::from(if value < 10 {
        b'0' + value
    } else {
        b'A' + value - 10
    })
}

#[cfg(test)]
mod tests {
    use super::expression_nodes;
    use crate::load_program_source;

    #[test]
    fn ids_ignore_whitespace_and_escape_definition_names() {
        let compact = load_program_source(
            "(program (name ids) (version 2) (def a/b (fn (x) (if x (+ x 1) 0))) (export a/b))",
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));
        let expanded = load_program_source(
            "(program\n  (name ids)\n  (version 2)\n  (def a/b\n    (fn (x)\n      (if x\n          (+ x 1)\n          0)))\n  (export a/b))\n",
        )
        .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

        let compact_ids = expression_nodes(&compact)
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>();
        let expanded_ids = expression_nodes(&expanded)
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>();
        assert_eq!(compact_ids, expanded_ids);
        assert!(compact_ids[0].contains("a%2Fb"));
    }
}
