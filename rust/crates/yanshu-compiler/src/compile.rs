#![forbid(unsafe_code)]

use yanshu_diagnostic::{Diagnostic, Span, YanshuResult};
use yanshu_syntax::{Datum, DatumKind, Expression, ExpressionKind, Program};

use crate::{CodeBlock, DefinitionCode, Instruction, LocatedInstruction};

pub(crate) struct Compilation {
    pub definitions: Vec<DefinitionCode>,
    pub blocks: Vec<CodeBlock>,
}

pub(crate) fn lower_program(program: &Program) -> YanshuResult<Compilation> {
    let mut compiler = Compiler { blocks: Vec::new() };
    let definitions = program
        .definitions
        .iter()
        .map(|definition| {
            compiler
                .compile_block(Vec::new(), &definition.expression)
                .map(|block| DefinitionCode {
                    name: definition.name.clone(),
                    block,
                })
        })
        .collect::<YanshuResult<Vec<_>>>()?;
    Ok(Compilation {
        definitions,
        blocks: compiler.blocks,
    })
}

struct Compiler {
    blocks: Vec<CodeBlock>,
}

impl Compiler {
    fn compile_block(
        &mut self,
        parameters: Vec<String>,
        expression: &Expression,
    ) -> YanshuResult<usize> {
        let identifier = self.blocks.len();
        self.blocks.push(CodeBlock {
            parameters: parameters.clone(),
            instructions: Vec::new(),
        });
        let mut instructions = Vec::new();
        self.compile_expression(expression, &mut instructions)?;
        emit(&mut instructions, Instruction::Return, expression.span);
        let Some(target) = self.blocks.get_mut(identifier) else {
            return Err(Diagnostic::simple(
                "COMPILER_INTERNAL",
                "compiler could not finalize a reserved code block",
            ));
        };
        *target = CodeBlock {
            parameters,
            instructions,
        };
        Ok(identifier)
    }

    #[allow(clippy::too_many_lines)]
    fn compile_expression(
        &mut self,
        expression: &Expression,
        instructions: &mut Vec<LocatedInstruction>,
    ) -> YanshuResult<()> {
        emit(instructions, Instruction::Charge, expression.span);
        match &expression.kind {
            ExpressionKind::Literal(value) | ExpressionKind::Quote(value) => {
                emit(
                    instructions,
                    Instruction::Constant(value.clone()),
                    expression.span,
                );
            }
            ExpressionKind::Variable(name) => emit(
                instructions,
                Instruction::Load(name.clone()),
                expression.span,
            ),
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => {
                self.compile_expression(condition, instructions)?;
                let false_jump = emit_jump(instructions, expression.span, ConditionalJump::False);
                self.compile_expression(consequent, instructions)?;
                let end_jump = emit_plain_jump(instructions, expression.span);
                let alternative_start = instructions.len();
                patch_target(instructions, false_jump, alternative_start)?;
                self.compile_expression(alternative, instructions)?;
                let end = instructions.len();
                patch_target(instructions, end_jump, end)?;
            }
            ExpressionKind::And(expressions) => {
                if expressions.is_empty() {
                    emit(
                        instructions,
                        Instruction::Constant(bool_datum(true, expression.span)),
                        expression.span,
                    );
                } else {
                    let mut jumps = Vec::new();
                    for item in &expressions[..expressions.len() - 1] {
                        self.compile_expression(item, instructions)?;
                        jumps.push(emit_jump(
                            instructions,
                            item.span,
                            ConditionalJump::FalseKeep,
                        ));
                    }
                    self.compile_expression(&expressions[expressions.len() - 1], instructions)?;
                    let end = instructions.len();
                    for jump in jumps {
                        patch_target(instructions, jump, end)?;
                    }
                }
            }
            ExpressionKind::Or(expressions) => {
                if expressions.is_empty() {
                    emit(
                        instructions,
                        Instruction::Constant(bool_datum(false, expression.span)),
                        expression.span,
                    );
                } else {
                    let mut jumps = Vec::new();
                    for item in &expressions[..expressions.len() - 1] {
                        self.compile_expression(item, instructions)?;
                        jumps.push(emit_jump(
                            instructions,
                            item.span,
                            ConditionalJump::TrueKeep,
                        ));
                    }
                    self.compile_expression(&expressions[expressions.len() - 1], instructions)?;
                    let end = instructions.len();
                    for jump in jumps {
                        patch_target(instructions, jump, end)?;
                    }
                }
            }
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => {
                let mut end_jumps = Vec::new();
                for clause in clauses {
                    self.compile_expression(&clause.condition, instructions)?;
                    let next =
                        emit_jump(instructions, clause.condition.span, ConditionalJump::False);
                    self.compile_expression(&clause.expression, instructions)?;
                    end_jumps.push(emit_plain_jump(instructions, clause.expression.span));
                    let next_clause = instructions.len();
                    patch_target(instructions, next, next_clause)?;
                }
                self.compile_expression(alternative, instructions)?;
                let end = instructions.len();
                for jump in end_jumps {
                    patch_target(instructions, jump, end)?;
                }
            }
            ExpressionKind::Match { value, arms } => {
                self.compile_expression(value, instructions)?;
                let mut end_jumps = Vec::new();
                for arm in arms {
                    let try_match = instructions.len();
                    emit(
                        instructions,
                        Instruction::TryMatch {
                            pattern: arm.pattern.clone(),
                            failure: 0,
                        },
                        arm.pattern.span,
                    );
                    self.compile_expression(&arm.expression, instructions)?;
                    emit(instructions, Instruction::ExitScope, arm.expression.span);
                    end_jumps.push(emit_plain_jump(instructions, arm.expression.span));
                    let next_arm = instructions.len();
                    patch_target(instructions, try_match, next_arm)?;
                }
                emit(instructions, Instruction::Pop, expression.span);
                emit(instructions, Instruction::MatchFail, expression.span);
                let end = instructions.len();
                for jump in end_jumps {
                    patch_target(instructions, jump, end)?;
                }
            }
            ExpressionKind::Let { bindings, body } => {
                emit(instructions, Instruction::EnterScope, expression.span);
                for binding in bindings {
                    self.compile_expression(&binding.expression, instructions)?;
                    emit(
                        instructions,
                        Instruction::Bind(binding.name.clone()),
                        binding.expression.span,
                    );
                }
                self.compile_expression(body, instructions)?;
                emit(instructions, Instruction::ExitScope, expression.span);
            }
            ExpressionKind::Function { parameters, body } => {
                let block = self.compile_block(parameters.clone(), body)?;
                emit(
                    instructions,
                    Instruction::MakeClosure(block),
                    expression.span,
                );
            }
            ExpressionKind::Do(expressions) => {
                if expressions.is_empty() {
                    emit(
                        instructions,
                        Instruction::Constant(nil_datum(expression.span)),
                        expression.span,
                    );
                } else {
                    for item in &expressions[..expressions.len() - 1] {
                        self.compile_expression(item, instructions)?;
                        emit(instructions, Instruction::Pop, item.span);
                    }
                    self.compile_expression(&expressions[expressions.len() - 1], instructions)?;
                }
            }
            ExpressionKind::Call { callee, arguments } => {
                self.compile_expression(callee, instructions)?;
                for argument in arguments {
                    self.compile_expression(argument, instructions)?;
                }
                emit(
                    instructions,
                    Instruction::Call(arguments.len()),
                    expression.span,
                );
            }
        }
        Ok(())
    }
}

fn emit(instructions: &mut Vec<LocatedInstruction>, instruction: Instruction, span: Span) {
    instructions.push(LocatedInstruction { instruction, span });
}

fn emit_plain_jump(instructions: &mut Vec<LocatedInstruction>, span: Span) -> usize {
    let index = instructions.len();
    emit(instructions, Instruction::Jump(0), span);
    index
}

fn emit_jump(
    instructions: &mut Vec<LocatedInstruction>,
    span: Span,
    kind: ConditionalJump,
) -> usize {
    let index = instructions.len();
    let instruction = match kind {
        ConditionalJump::False => Instruction::JumpIfFalse(0),
        ConditionalJump::FalseKeep => Instruction::JumpIfFalseKeep(0),
        ConditionalJump::TrueKeep => Instruction::JumpIfTrueKeep(0),
    };
    emit(instructions, instruction, span);
    index
}

#[derive(Debug, Clone, Copy)]
enum ConditionalJump {
    False,
    FalseKeep,
    TrueKeep,
}

fn patch_target(
    instructions: &mut [LocatedInstruction],
    index: usize,
    target: usize,
) -> YanshuResult<()> {
    let Some(located) = instructions.get_mut(index) else {
        return Err(Diagnostic::simple(
            "COMPILER_INTERNAL",
            "compiler could not patch a missing jump",
        ));
    };
    match &mut located.instruction {
        Instruction::Jump(value)
        | Instruction::JumpIfFalse(value)
        | Instruction::JumpIfFalseKeep(value)
        | Instruction::JumpIfTrueKeep(value) => *value = target,
        Instruction::TryMatch { failure, .. } => *failure = target,
        _ => {
            return Err(Diagnostic::simple(
                "COMPILER_INTERNAL",
                "compiler attempted to patch a non-branch instruction",
            ));
        }
    }
    Ok(())
}

fn bool_datum(value: bool, span: Span) -> Datum {
    Datum {
        kind: DatumKind::Bool(value),
        span,
    }
}

fn nil_datum(span: Span) -> Datum {
    Datum {
        kind: DatumKind::List(Vec::new()),
        span,
    }
}
