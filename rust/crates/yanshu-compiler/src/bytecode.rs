#![forbid(unsafe_code)]

use serde_json::{Value, json};
use yanshu_diagnostic::Span;
use yanshu_syntax::{Datum, Pattern};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Charge,
    Constant(Datum),
    Load(String),
    Pop,
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfFalseKeep(usize),
    JumpIfTrueKeep(usize),
    EnterScope,
    Bind(String),
    ExitScope,
    MakeClosure(usize),
    Call(usize),
    TryMatch { pattern: Pattern, failure: usize },
    MatchFail,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedInstruction {
    pub instruction: Instruction,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    pub parameters: Vec<String>,
    pub instructions: Vec<LocatedInstruction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionCode {
    pub name: String,
    pub block: usize,
}

impl Instruction {
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Charge => json!({ "op": "charge" }),
            Self::Constant(value) => json!({ "op": "constant", "value": value.portable_json() }),
            Self::Load(name) => json!({ "op": "load", "name": name }),
            Self::Pop => json!({ "op": "pop" }),
            Self::Jump(target) => json!({ "op": "jump", "target": target }),
            Self::JumpIfFalse(target) => json!({ "op": "jump-if-false", "target": target }),
            Self::JumpIfFalseKeep(target) => {
                json!({ "op": "jump-if-false-keep", "target": target })
            }
            Self::JumpIfTrueKeep(target) => {
                json!({ "op": "jump-if-true-keep", "target": target })
            }
            Self::EnterScope => json!({ "op": "enter-scope" }),
            Self::Bind(name) => json!({ "op": "bind", "name": name }),
            Self::ExitScope => json!({ "op": "exit-scope" }),
            Self::MakeClosure(block) => json!({ "op": "make-closure", "block": block }),
            Self::Call(arity) => json!({ "op": "call", "arity": arity }),
            Self::TryMatch { pattern, failure } => json!({
                "op": "try-match",
                "pattern": pattern.to_json(),
                "failure": failure,
            }),
            Self::MatchFail => json!({ "op": "match-fail" }),
            Self::Return => json!({ "op": "return" }),
        }
    }

    #[must_use]
    pub const fn fuel_cost(&self) -> u64 {
        match self {
            Self::Charge => 1,
            _ => 0,
        }
    }
}

impl LocatedInstruction {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "instruction": self.instruction.to_json(),
            "span": span_json(self.span),
        })
    }
}

impl CodeBlock {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "parameters": self.parameters,
            "instructions": self.instructions.iter().map(LocatedInstruction::to_json).collect::<Vec<_>>(),
        })
    }
}

impl DefinitionCode {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "block": self.block })
    }
}

fn span_json(span: Span) -> Value {
    json!({
        "start": {
            "offset": span.start.offset,
            "line": span.start.line,
            "column": span.start.column,
        },
        "end": {
            "offset": span.end.offset,
            "line": span.end.line,
            "column": span.end.column,
        },
    })
}
