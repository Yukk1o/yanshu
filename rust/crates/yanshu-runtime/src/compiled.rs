#![forbid(unsafe_code)]

use serde_json::json;
use yanshu_compiler::{CodeBlock, Instruction};
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{Closure, ClosureBody, Runtime, Value, matcher::bindings_for_pattern};

impl Runtime<'_, '_, '_> {
    #[allow(clippy::too_many_lines)]
    pub(super) fn execute_block(
        &mut self,
        block: &CodeBlock,
        environment: usize,
        depth: usize,
    ) -> YanshuResult<Value> {
        self.budget.check_depth(depth)?;
        let mut stack = Vec::new();
        let mut current_environment = environment;
        let mut scope_stack = Vec::new();
        let mut pc = 0_usize;
        loop {
            let located = block.instructions.get(pc).ok_or_else(|| {
                Diagnostic::new(
                    "BYTECODE_FALLTHROUGH",
                    "bytecode execution left its verified code block",
                    json!({ "pc": pc }),
                )
            })?;
            self.budget.consume(located.instruction.fuel_cost())?;
            match &located.instruction {
                Instruction::Charge => {}
                Instruction::Constant(datum) => {
                    self.charge_datum(datum)?;
                    stack.push(Value::from(datum));
                }
                Instruction::Load(name) => stack.push(self.lookup(current_environment, name)?),
                Instruction::Pop => {
                    pop_bytecode_value(&mut stack, pc)?;
                }
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    let condition = pop_bytecode_value(&mut stack, pc)?;
                    if !condition.truthy() {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::JumpIfFalseKeep(target) => {
                    let condition = peek_bytecode_value(&stack, pc)?;
                    if !condition.truthy() {
                        pc = *target;
                        continue;
                    }
                    pop_bytecode_value(&mut stack, pc)?;
                }
                Instruction::JumpIfTrueKeep(target) => {
                    let condition = peek_bytecode_value(&stack, pc)?;
                    if condition.truthy() {
                        pc = *target;
                        continue;
                    }
                    pop_bytecode_value(&mut stack, pc)?;
                }
                Instruction::EnterScope => {
                    let local = self.new_environment(Some(current_environment));
                    scope_stack.push(current_environment);
                    current_environment = local;
                }
                Instruction::Bind(name) => {
                    let value = pop_bytecode_value(&mut stack, pc)?;
                    self.define(current_environment, name.clone(), value);
                }
                Instruction::ExitScope => {
                    current_environment = scope_stack.pop().ok_or_else(|| {
                        Diagnostic::new(
                            "BYTECODE_SCOPE_UNDERFLOW",
                            "bytecode execution exited a missing scope",
                            json!({ "pc": pc }),
                        )
                    })?;
                }
                Instruction::MakeClosure(identifier) => {
                    let parameters = self
                        .bytecode
                        .and_then(|artifact| artifact.blocks().get(*identifier))
                        .map(|target| target.parameters.clone())
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "BYTECODE_UNKNOWN_BLOCK",
                                "bytecode execution referenced an unknown closure block",
                                json!({ "block": identifier }),
                            )
                        })?;
                    let closure = self.closures.len();
                    self.closures.push(Closure {
                        parameters,
                        body: ClosureBody::Bytecode(*identifier),
                        environment: current_environment,
                    });
                    stack.push(Value::Closure(closure));
                }
                Instruction::Call(arity) => {
                    let required = arity.checked_add(1).ok_or_else(|| {
                        Diagnostic::simple("BYTECODE_ARITY_LIMIT", "bytecode arity overflowed")
                    })?;
                    if stack.len() < required {
                        return Err(bytecode_stack_underflow(pc));
                    }
                    let first_argument = stack.len() - arity;
                    let arguments = stack.split_off(first_argument);
                    let callable = pop_bytecode_value(&mut stack, pc)?;
                    let result = self.apply(callable, arguments, depth + 1)?;
                    stack.push(result);
                }
                Instruction::TryMatch { pattern, failure } => {
                    let value = peek_bytecode_value(&stack, pc)?;
                    if let Some(bindings) = bindings_for_pattern(pattern, value, &mut self.budget)?
                    {
                        pop_bytecode_value(&mut stack, pc)?;
                        let local = self.new_environment(Some(current_environment));
                        for (name, value) in bindings {
                            self.define(local, name, value);
                        }
                        scope_stack.push(current_environment);
                        current_environment = local;
                    } else {
                        pc = *failure;
                        continue;
                    }
                }
                Instruction::MatchFail => {
                    return Err(Diagnostic::simple(
                        "RUNTIME_MATCH_NOT_EXHAUSTIVE",
                        "match did not select an arm",
                    ));
                }
                Instruction::Return => {
                    if stack.len() != 1 || !scope_stack.is_empty() {
                        return Err(Diagnostic::new(
                            "BYTECODE_RETURN_STATE",
                            "bytecode returned with an invalid stack or scope state",
                            json!({
                                "pc": pc,
                                "stack": stack.len(),
                                "scopes": scope_stack.len(),
                            }),
                        ));
                    }
                    return pop_bytecode_value(&mut stack, pc);
                }
            }
            pc = pc.checked_add(1).ok_or_else(|| {
                Diagnostic::simple(
                    "BYTECODE_PROGRAM_COUNTER_OVERFLOW",
                    "bytecode program counter overflowed",
                )
            })?;
        }
    }
}

fn peek_bytecode_value(stack: &[Value], pc: usize) -> YanshuResult<&Value> {
    stack.last().ok_or_else(|| bytecode_stack_underflow(pc))
}

fn pop_bytecode_value(stack: &mut Vec<Value>, pc: usize) -> YanshuResult<Value> {
    stack.pop().ok_or_else(|| bytecode_stack_underflow(pc))
}

fn bytecode_stack_underflow(pc: usize) -> Diagnostic {
    Diagnostic::new(
        "BYTECODE_STACK_UNDERFLOW",
        "bytecode execution required a missing stack value",
        json!({ "pc": pc }),
    )
}
