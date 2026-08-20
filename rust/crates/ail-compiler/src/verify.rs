#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::json;

use crate::{BytecodeArtifact, CodeBlock, Instruction};

const MAXIMUM_BLOCKS: usize = 4_096;
const MAXIMUM_INSTRUCTIONS: usize = 1_000_000;
const MAXIMUM_NAME_BYTES: usize = 256;

pub fn verify_bytecode(artifact: &BytecodeArtifact) -> AilResult<()> {
    if artifact.blocks().is_empty() || artifact.blocks().len() > MAXIMUM_BLOCKS {
        return Err(Diagnostic::new(
            "BYTECODE_BLOCK_LIMIT",
            "bytecode artifact has an invalid number of code blocks",
            json!({ "maximum": MAXIMUM_BLOCKS, "actual": artifact.blocks().len() }),
        ));
    }
    let instruction_count = artifact.blocks().iter().try_fold(0_usize, |count, block| {
        count.checked_add(block.instructions.len()).ok_or_else(|| {
            Diagnostic::simple(
                "BYTECODE_INSTRUCTION_LIMIT",
                "bytecode instruction count overflowed",
            )
        })
    })?;
    if instruction_count > MAXIMUM_INSTRUCTIONS {
        return Err(Diagnostic::new(
            "BYTECODE_INSTRUCTION_LIMIT",
            "bytecode artifact exceeds the instruction limit",
            json!({ "maximum": MAXIMUM_INSTRUCTIONS, "actual": instruction_count }),
        ));
    }

    let mut definitions = BTreeMap::new();
    for definition in artifact.definitions() {
        validate_name(&definition.name)?;
        if definition.block >= artifact.blocks().len() {
            return Err(Diagnostic::new(
                "BYTECODE_UNKNOWN_BLOCK",
                "definition references an unknown code block",
                json!({ "definition": definition.name, "block": definition.block }),
            ));
        }
        if definitions
            .insert(definition.name.clone(), definition.block)
            .is_some()
        {
            return Err(Diagnostic::new(
                "BYTECODE_DUPLICATE_DEFINITION",
                "bytecode artifact repeats a definition",
                json!({ "definition": definition.name }),
            ));
        }
    }
    for export in artifact.exports() {
        validate_name(export)?;
        if !definitions.contains_key(export) {
            return Err(Diagnostic::new(
                "BYTECODE_EXPORT_MISSING",
                "bytecode export has no compiled definition",
                json!({ "export": export }),
            ));
        }
    }
    for (identifier, block) in artifact.blocks().iter().enumerate() {
        verify_block(identifier, block, artifact.blocks().len())?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    stack: usize,
    scopes: usize,
}

fn verify_block(identifier: usize, block: &CodeBlock, block_count: usize) -> AilResult<()> {
    for parameter in &block.parameters {
        validate_name(parameter)?;
    }
    if block.instructions.is_empty() {
        return Err(Diagnostic::new(
            "BYTECODE_EMPTY_BLOCK",
            "bytecode code block cannot be empty",
            json!({ "block": identifier }),
        ));
    }
    let mut states = BTreeMap::new();
    let mut pending = VecDeque::from([(
        0_usize,
        State {
            stack: 0,
            scopes: 0,
        },
    )]);
    let mut saw_return = false;
    while let Some((pc, state)) = pending.pop_front() {
        if pc >= block.instructions.len() {
            return Err(Diagnostic::new(
                "BYTECODE_FALLTHROUGH",
                "bytecode control flow falls beyond its code block",
                json!({ "block": identifier, "pc": pc }),
            ));
        }
        if let Some(existing) = states.get(&pc) {
            if *existing != state {
                return Err(Diagnostic::new(
                    "BYTECODE_STATE_MERGE",
                    "bytecode control-flow paths disagree on stack or scope depth",
                    json!({ "block": identifier, "pc": pc }),
                ));
            }
            continue;
        }
        states.insert(pc, state);
        let instruction = &block.instructions[pc].instruction;
        match instruction {
            Instruction::Charge => enqueue(&mut pending, pc + 1, state)?,
            Instruction::Constant(_) | Instruction::Load(_) | Instruction::MakeClosure(_) => {
                if let Instruction::Load(name) = instruction {
                    validate_name(name)?;
                }
                if let Instruction::MakeClosure(target) = instruction
                    && *target >= block_count
                {
                    return Err(unknown_block(identifier, pc, *target));
                }
                enqueue(&mut pending, pc + 1, pushed(state, identifier, pc)?)?;
            }
            Instruction::Pop | Instruction::Bind(_) => {
                if let Instruction::Bind(name) = instruction {
                    validate_name(name)?;
                }
                enqueue(&mut pending, pc + 1, popped(state, identifier, pc)?)?;
            }
            Instruction::Jump(target) => {
                check_target(identifier, pc, *target, block.instructions.len())?;
                enqueue(&mut pending, *target, state)?;
            }
            Instruction::JumpIfFalse(target) => {
                check_target(identifier, pc, *target, block.instructions.len())?;
                let next = popped(state, identifier, pc)?;
                enqueue(&mut pending, *target, next)?;
                enqueue(&mut pending, pc + 1, next)?;
            }
            Instruction::JumpIfFalseKeep(target) | Instruction::JumpIfTrueKeep(target) => {
                check_target(identifier, pc, *target, block.instructions.len())?;
                require_stack(state, 1, identifier, pc)?;
                enqueue(&mut pending, *target, state)?;
                enqueue(&mut pending, pc + 1, popped(state, identifier, pc)?)?;
            }
            Instruction::EnterScope => enqueue(
                &mut pending,
                pc + 1,
                State {
                    scopes: state.scopes.checked_add(1).ok_or_else(|| {
                        Diagnostic::simple(
                            "BYTECODE_SCOPE_OVERFLOW",
                            "bytecode scope depth overflowed",
                        )
                    })?,
                    ..state
                },
            )?,
            Instruction::ExitScope => {
                if state.scopes == 0 {
                    return Err(Diagnostic::new(
                        "BYTECODE_SCOPE_UNDERFLOW",
                        "bytecode exits a scope that was not entered",
                        json!({ "block": identifier, "pc": pc }),
                    ));
                }
                enqueue(
                    &mut pending,
                    pc + 1,
                    State {
                        scopes: state.scopes - 1,
                        ..state
                    },
                )?;
            }
            Instruction::Call(arity) => {
                let required = arity.checked_add(1).ok_or_else(|| {
                    Diagnostic::simple("BYTECODE_ARITY_LIMIT", "bytecode call arity overflowed")
                })?;
                require_stack(state, required, identifier, pc)?;
                enqueue(
                    &mut pending,
                    pc + 1,
                    State {
                        stack: state.stack - arity,
                        ..state
                    },
                )?;
            }
            Instruction::TryMatch { failure, .. } => {
                check_target(identifier, pc, *failure, block.instructions.len())?;
                require_stack(state, 1, identifier, pc)?;
                enqueue(&mut pending, *failure, state)?;
                enqueue(
                    &mut pending,
                    pc + 1,
                    State {
                        stack: state.stack - 1,
                        scopes: state.scopes.checked_add(1).ok_or_else(|| {
                            Diagnostic::simple(
                                "BYTECODE_SCOPE_OVERFLOW",
                                "bytecode match scope depth overflowed",
                            )
                        })?,
                    },
                )?;
            }
            Instruction::MatchFail => {
                if state.stack != 0 || state.scopes != 0 {
                    return Err(Diagnostic::new(
                        "BYTECODE_MATCH_STATE",
                        "failed match must terminate with an empty stack and no local scopes",
                        json!({ "block": identifier, "pc": pc }),
                    ));
                }
            }
            Instruction::Return => {
                saw_return = true;
                if state.stack != 1 || state.scopes != 0 {
                    return Err(Diagnostic::new(
                        "BYTECODE_RETURN_STATE",
                        "bytecode return requires one value and no local scopes",
                        json!({
                            "block": identifier,
                            "pc": pc,
                            "stack": state.stack,
                            "scopes": state.scopes,
                        }),
                    ));
                }
            }
        }
    }
    if !saw_return {
        return Err(Diagnostic::new(
            "BYTECODE_RETURN_MISSING",
            "reachable bytecode path never returns",
            json!({ "block": identifier }),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> AilResult<()> {
    if name.is_empty() || name.len() > MAXIMUM_NAME_BYTES || name.chars().any(char::is_control) {
        Err(Diagnostic::new(
            "BYTECODE_INVALID_NAME",
            "bytecode name is empty, oversized, or contains control characters",
            json!({ "maximumBytes": MAXIMUM_NAME_BYTES }),
        ))
    } else {
        Ok(())
    }
}

fn check_target(block: usize, pc: usize, target: usize, length: usize) -> AilResult<()> {
    if target >= length {
        Err(Diagnostic::new(
            "BYTECODE_INVALID_JUMP",
            "bytecode jump target is outside its code block",
            json!({ "block": block, "pc": pc, "target": target, "length": length }),
        ))
    } else {
        Ok(())
    }
}

fn require_stack(state: State, required: usize, block: usize, pc: usize) -> AilResult<()> {
    if state.stack < required {
        Err(Diagnostic::new(
            "BYTECODE_STACK_UNDERFLOW",
            "bytecode instruction requires more stack values",
            json!({ "block": block, "pc": pc, "required": required, "actual": state.stack }),
        ))
    } else {
        Ok(())
    }
}

fn pushed(state: State, block: usize, pc: usize) -> AilResult<State> {
    Ok(State {
        stack: state.stack.checked_add(1).ok_or_else(|| {
            Diagnostic::new(
                "BYTECODE_STACK_OVERFLOW",
                "bytecode stack depth overflowed",
                json!({ "block": block, "pc": pc }),
            )
        })?,
        ..state
    })
}

fn popped(state: State, block: usize, pc: usize) -> AilResult<State> {
    require_stack(state, 1, block, pc)?;
    Ok(State {
        stack: state.stack - 1,
        ..state
    })
}

fn enqueue(pending: &mut VecDeque<(usize, State)>, pc: usize, state: State) -> AilResult<()> {
    if pending.len() >= MAXIMUM_INSTRUCTIONS.saturating_mul(2) {
        return Err(Diagnostic::simple(
            "BYTECODE_CONTROL_FLOW_LIMIT",
            "bytecode verifier control-flow queue exceeded its limit",
        ));
    }
    pending.push_back((pc, state));
    Ok(())
}

fn unknown_block(block: usize, pc: usize, target: usize) -> Diagnostic {
    Diagnostic::new(
        "BYTECODE_UNKNOWN_BLOCK",
        "closure instruction references an unknown code block",
        json!({ "block": block, "pc": pc, "target": target }),
    )
}
