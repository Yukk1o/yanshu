#![forbid(unsafe_code)]

mod budget;
mod compiled;
mod matcher;
mod schema;
mod value;

use std::{collections::BTreeMap, sync::Arc};

use num_bigint::BigInt;
use num_traits::{One, Zero};
use serde_json::{Value as JsonValue, json};
use yanshu_analysis::analyze_program;
use yanshu_compiler::{BytecodeArtifact, verify_bytecode};
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_library::{LibraryKey, LibraryRegistry, LibraryValue};
use yanshu_syntax::{Expression, ExpressionKind, Program, TypeExpression};

pub use budget::Budget;
pub use value::{MapKey, Primitive, PrimitiveOperation, Value, bigint_json, json_to_value};

use matcher::bindings_for_pattern;
use schema::validate_schema;
use value::{
    MAXIMUM_INTEGER_BITS, MAXIMUM_VALUE_BYTES, measure_datum, measure_portable_value,
    measure_runtime_value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionOptions {
    pub fuel: u64,
    pub maximum_depth: usize,
    pub reference_libraries: bool,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            fuel: 10_000,
            maximum_depth: 256,
            reference_libraries: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub value: Value,
    pub fuel_limit: u64,
    pub fuel_consumed: u64,
    pub fuel_remaining: u64,
}

impl ExecutionReport {
    #[must_use]
    pub fn cost_json(&self) -> JsonValue {
        json!({
            "fuelLimit": self.fuel_limit,
            "fuelConsumed": self.fuel_consumed,
            "fuelRemaining": self.fuel_remaining,
        })
    }
}

pub fn execute_export(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
) -> YanshuResult<Value> {
    let mut libraries = default_library_registry(options);
    execute_export_internal(
        program,
        export_name,
        arguments,
        options,
        None,
        &mut libraries,
    )
}

pub fn execute_compiled_export(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
) -> YanshuResult<Value> {
    execute_compiled_export_report(artifact, export_name, arguments, options)
        .map(|report| report.value)
}

pub fn execute_compiled_export_report(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
) -> YanshuResult<ExecutionReport> {
    let mut libraries = default_library_registry(options);
    execute_compiled_export_internal(
        artifact,
        export_name,
        arguments,
        options,
        None,
        &mut libraries,
    )
}

pub fn execute_compiled_export_with_host(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: &mut dyn CapabilityHost,
) -> YanshuResult<Value> {
    let mut libraries = default_library_registry(options);
    execute_compiled_export_internal(
        artifact,
        export_name,
        arguments,
        options,
        Some(host),
        &mut libraries,
    )
    .map(|report| report.value)
}

pub fn execute_compiled_export_with_host_report(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: &mut dyn CapabilityHost,
) -> YanshuResult<ExecutionReport> {
    let mut libraries = default_library_registry(options);
    execute_compiled_export_internal(
        artifact,
        export_name,
        arguments,
        options,
        Some(host),
        &mut libraries,
    )
}

pub fn execute_compiled_export_with_libraries(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<Value> {
    execute_compiled_export_internal(artifact, export_name, arguments, options, None, libraries)
        .map(|report| report.value)
}

pub fn execute_compiled_export_with_host_and_libraries(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: &mut dyn CapabilityHost,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<Value> {
    execute_compiled_export_internal(
        artifact,
        export_name,
        arguments,
        options,
        Some(host),
        libraries,
    )
    .map(|report| report.value)
}

pub trait CapabilityHost {
    fn supports(&self, capability: &str) -> bool;
    fn invoke(&mut self, operation: &str, arguments: &[Value]) -> YanshuResult<Value>;
}

pub fn execute_export_with_host(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: &mut dyn CapabilityHost,
) -> YanshuResult<Value> {
    let mut libraries = default_library_registry(options);
    execute_export_internal(
        program,
        export_name,
        arguments,
        options,
        Some(host),
        &mut libraries,
    )
}

pub fn execute_export_with_libraries(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<Value> {
    execute_export_internal(program, export_name, arguments, options, None, libraries)
}

pub fn execute_export_with_host_and_libraries(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: &mut dyn CapabilityHost,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<Value> {
    execute_export_internal(
        program,
        export_name,
        arguments,
        options,
        Some(host),
        libraries,
    )
}

fn execute_export_internal(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: Option<&mut dyn CapabilityHost>,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<Value> {
    validate_boundary_arguments(&arguments)?;
    if !program.imports.is_empty() {
        return Err(Diagnostic::new(
            "RUNTIME_UNLINKED_IMPORTS",
            "program imports must be linked from a sealed bundle before execution",
            json!({ "imports": program.imports }),
        ));
    }
    if !program.exports.iter().any(|name| name == export_name) {
        return Err(Diagnostic::new(
            "RUNTIME_NOT_EXPORTED",
            "requested entry point is not exported",
            json!({ "name": export_name }),
        ));
    }
    if program.version >= BigInt::from(4_u8) {
        analyze_program(program)?;
        validate_export_arguments(program, export_name, &arguments)?;
    }
    let mut runtime = Runtime::new(options, host, libraries, None);
    let module_environment = runtime.install_program_environment(program)?;
    for definition in &program.definitions {
        let value = runtime.evaluate(&definition.expression, module_environment, 0)?;
        runtime.define(module_environment, definition.name.clone(), value);
    }
    let callable = runtime.lookup(module_environment, export_name)?;
    let result = runtime.apply(callable, arguments, 0)?;
    if program.version >= BigInt::from(4_u8) {
        validate_export_result(program, export_name, &result)?;
    }
    Ok(result)
}

fn execute_compiled_export_internal(
    artifact: &BytecodeArtifact,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
    host: Option<&mut dyn CapabilityHost>,
    libraries: &mut LibraryRegistry,
) -> YanshuResult<ExecutionReport> {
    validate_boundary_arguments(&arguments)?;
    verify_bytecode(artifact)?;
    let program = artifact.program();
    if !program.exports.iter().any(|name| name == export_name) {
        return Err(Diagnostic::new(
            "RUNTIME_NOT_EXPORTED",
            "requested entry point is not exported",
            json!({ "name": export_name }),
        ));
    }
    validate_export_arguments(program, export_name, &arguments)?;
    let mut runtime = Runtime::new(options, host, libraries, Some(artifact));
    let module_environment = runtime.install_program_environment(program)?;
    for definition in artifact.definitions() {
        let block = runtime.compiled_block(definition.block)?;
        let value = runtime.execute_block(&block, module_environment, 0)?;
        runtime.define(module_environment, definition.name.clone(), value);
    }
    let callable = runtime.lookup(module_environment, export_name)?;
    let result = runtime.apply(callable, arguments, 0)?;
    validate_export_result(program, export_name, &result)?;
    let fuel_remaining = runtime.budget.remaining_fuel();
    Ok(ExecutionReport {
        value: result,
        fuel_limit: options.fuel,
        fuel_consumed: options.fuel.saturating_sub(fuel_remaining),
        fuel_remaining,
    })
}

fn default_library_registry(options: ExecutionOptions) -> LibraryRegistry {
    if options.reference_libraries {
        LibraryRegistry::rust_standard()
    } else {
        LibraryRegistry::default()
    }
}

fn validate_boundary_arguments(arguments: &[Value]) -> YanshuResult<()> {
    for argument in arguments {
        measure_portable_value(argument)?;
    }
    Ok(())
}

fn validate_capability_result(operation: &str, value: &Value) -> YanshuResult<()> {
    let valid = match operation {
        "log" | "kv-put" => matches!(value, Value::Nil),
        "now-ms" => matches!(value, Value::Int(_)),
        "kv-delete" => matches!(value, Value::Bool(_)),
        "kv-list" => matches!(value, Value::Nil | Value::List(_)),
        "kv-get" => true,
        _ => false,
    };
    if valid {
        return Ok(());
    }
    Err(Diagnostic::new(
        "RUNTIME_CAPABILITY_CONTRACT_FAILURE",
        "host capability returned a value outside its language contract",
        json!({ "operation": operation, "actual": value.kind() }),
    ))
}

fn validate_export_arguments(
    program: &Program,
    export_name: &str,
    arguments: &[Value],
) -> YanshuResult<()> {
    let signature = program
        .signatures
        .iter()
        .find(|signature| signature.name == export_name)
        .ok_or_else(|| {
            Diagnostic::new(
                "TYPE_EXPORT_SIGNATURE_MISSING",
                "exported v4 function has no linked signature",
                json!({ "name": export_name }),
            )
        })?;
    if signature.parameters.len() != arguments.len() {
        return Err(arity_error(
            export_name,
            signature.parameters.len(),
            Some(signature.parameters.len()),
            arguments.len(),
        ));
    }
    for (index, (expected, actual)) in signature.parameters.iter().zip(arguments).enumerate() {
        if !value_matches_type(actual, expected) {
            return Err(Diagnostic::new(
                "TYPE_INPUT_MISMATCH",
                "host argument does not satisfy the exported function signature",
                json!({
                    "name": export_name,
                    "index": index,
                    "expected": expected.to_json(),
                    "actual": actual.kind(),
                }),
            ));
        }
    }
    Ok(())
}

fn validate_export_result(
    program: &Program,
    export_name: &str,
    result: &Value,
) -> YanshuResult<()> {
    let signature = program
        .signatures
        .iter()
        .find(|signature| signature.name == export_name)
        .ok_or_else(|| {
            Diagnostic::new(
                "TYPE_EXPORT_SIGNATURE_MISSING",
                "exported v4 function has no linked signature",
                json!({ "name": export_name }),
            )
        })?;
    if value_matches_type(result, &signature.result) {
        Ok(())
    } else {
        Err(Diagnostic::new(
            "TYPE_OUTPUT_MISMATCH",
            "guest result does not satisfy the exported function signature",
            json!({
                "name": export_name,
                "expected": signature.result.to_json(),
                "actual": result.kind(),
            }),
        ))
    }
}

fn value_matches_type(value: &Value, expected: &TypeExpression) -> bool {
    match expected {
        TypeExpression::Named(name) => match name.as_str() {
            "any" => true,
            "integer" => matches!(value, Value::Int(_)),
            "boolean" => matches!(value, Value::Bool(_)),
            "string" => matches!(value, Value::String(_)),
            "symbol" => matches!(value, Value::Symbol(_)),
            "nil" => matches!(value, Value::Nil),
            "map" => matches!(value, Value::Map(_)),
            user_type => {
                matches!(value, Value::Variant { type_name, .. } if type_name == user_type)
            }
        },
        TypeExpression::List(item) => match value {
            Value::Nil => true,
            Value::List(values) => values.iter().all(|value| value_matches_type(value, item)),
            _ => false,
        },
        TypeExpression::Result { success, error } => match value {
            Value::Ok(value) => value_matches_type(value, success),
            Value::Err(value) => value_matches_type(value, error),
            _ => false,
        },
        TypeExpression::Function { .. } => matches!(
            value,
            Value::Closure(_)
                | Value::Primitive(_)
                | Value::Constructor { .. }
                | Value::LibraryFunction { .. }
        ),
    }
}

#[derive(Debug, Clone)]
struct Environment {
    bindings: BTreeMap<String, Value>,
    parent: Option<usize>,
}

#[derive(Debug, Clone)]
struct Closure {
    parameters: Vec<String>,
    body: ClosureBody,
    environment: usize,
}

#[derive(Debug, Clone)]
enum ClosureBody {
    Interpreted(Arc<Expression>),
    Bytecode(usize),
}

struct Runtime<'host, 'libraries, 'bytecode> {
    budget: Budget,
    environments: Vec<Environment>,
    closures: Vec<Closure>,
    host: Option<&'host mut dyn CapabilityHost>,
    libraries: &'libraries mut LibraryRegistry,
    bytecode: Option<&'bytecode BytecodeArtifact>,
    interpreted_bodies: BTreeMap<usize, Arc<Expression>>,
    compiled_blocks: BTreeMap<usize, Arc<yanshu_compiler::CodeBlock>>,
}

impl<'host, 'libraries, 'bytecode> Runtime<'host, 'libraries, 'bytecode> {
    fn new(
        options: ExecutionOptions,
        host: Option<&'host mut dyn CapabilityHost>,
        libraries: &'libraries mut LibraryRegistry,
        bytecode: Option<&'bytecode BytecodeArtifact>,
    ) -> Self {
        Self {
            budget: Budget::new(options.fuel, options.maximum_depth),
            environments: Vec::new(),
            closures: Vec::new(),
            host,
            libraries,
            bytecode,
            interpreted_bodies: BTreeMap::new(),
            compiled_blocks: BTreeMap::new(),
        }
    }

    fn interpreted_body(&mut self, body: &Expression) -> Arc<Expression> {
        let key = std::ptr::from_ref(body).addr();
        self.interpreted_bodies
            .entry(key)
            .or_insert_with(|| Arc::new(body.clone()))
            .clone()
    }

    fn compiled_block(
        &mut self,
        identifier: usize,
    ) -> YanshuResult<Arc<yanshu_compiler::CodeBlock>> {
        if let Some(block) = self.compiled_blocks.get(&identifier) {
            return Ok(Arc::clone(block));
        }
        let block = self
            .bytecode
            .and_then(|artifact| artifact.blocks().get(identifier))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::new(
                    "BYTECODE_UNKNOWN_BLOCK",
                    "compiled closure references an unknown code block",
                    json!({ "block": identifier }),
                )
            })?;
        let block = Arc::new(block);
        self.compiled_blocks.insert(identifier, Arc::clone(&block));
        Ok(block)
    }

    fn install_program_environment(&mut self, program: &Program) -> YanshuResult<usize> {
        let base_environment = self.new_environment(None);
        self.install_base_environment(program, base_environment);
        let module_environment = self.new_environment(Some(base_environment));
        self.install_libraries(program, module_environment)?;
        self.install_capabilities(program, module_environment)?;
        for schema in &program.schemas {
            self.define(
                module_environment,
                schema.name.clone(),
                Value::Schema {
                    name: schema.name.clone(),
                    specification: schema.kind.clone(),
                },
            );
        }
        for data_type in &program.data_types {
            for variant in &data_type.variants {
                self.define(
                    module_environment,
                    variant.name.clone(),
                    Value::Constructor {
                        type_name: data_type.name.clone(),
                        variant: variant.name.clone(),
                        arity: variant.fields.len(),
                    },
                );
            }
        }
        Ok(module_environment)
    }

    fn new_environment(&mut self, parent: Option<usize>) -> usize {
        let identifier = self.environments.len();
        self.environments.push(Environment {
            bindings: BTreeMap::new(),
            parent,
        });
        identifier
    }

    fn define(&mut self, environment: usize, name: String, value: Value) {
        if let Some(target) = self.environments.get_mut(environment) {
            target.bindings.insert(name, value);
        }
    }

    fn lookup(&mut self, environment: usize, name: &str) -> YanshuResult<Value> {
        let mut current = Some(environment);
        while let Some(identifier) = current {
            let Some(target) = self.environments.get(identifier) else {
                break;
            };
            if target.bindings.contains_key(name) {
                let metrics = {
                    let value = self
                        .environments
                        .get(identifier)
                        .and_then(|environment| environment.bindings.get(name))
                        .ok_or_else(|| {
                            Diagnostic::simple(
                                "RUNTIME_ENVIRONMENT_CORRUPT",
                                "runtime environment changed during lookup",
                            )
                        })?;
                    measure_runtime_value(value)?
                };
                self.budget.consume(metrics.fuel_cost())?;
                return self
                    .environments
                    .get(identifier)
                    .and_then(|environment| environment.bindings.get(name))
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::simple(
                            "RUNTIME_ENVIRONMENT_CORRUPT",
                            "runtime environment changed during lookup",
                        )
                    });
            }
            current = target.parent;
        }
        Err(Diagnostic::new(
            "RUNTIME_UNBOUND_NAME",
            "name is not bound in the current environment",
            json!({ "name": name }),
        ))
    }

    fn evaluate(
        &mut self,
        expression: &Expression,
        environment: usize,
        depth: usize,
    ) -> YanshuResult<Value> {
        self.budget.consume(1)?;
        self.budget.check_depth(depth)?;
        match &expression.kind {
            ExpressionKind::Literal(datum) | ExpressionKind::Quote(datum) => {
                self.charge_datum(datum)?;
                Ok(Value::from(datum))
            }
            ExpressionKind::Variable(name) => self.lookup(environment, name),
            ExpressionKind::If {
                condition,
                consequent,
                alternative,
            } => {
                if self.evaluate(condition, environment, depth)?.truthy() {
                    self.evaluate(consequent, environment, depth)
                } else {
                    self.evaluate(alternative, environment, depth)
                }
            }
            ExpressionKind::And(expressions) => {
                let mut result = Value::Bool(true);
                for item in expressions {
                    result = self.evaluate(item, environment, depth)?;
                    if !result.truthy() {
                        return Ok(result);
                    }
                }
                Ok(result)
            }
            ExpressionKind::Or(expressions) => {
                for item in expressions {
                    let result = self.evaluate(item, environment, depth)?;
                    if result.truthy() {
                        return Ok(result);
                    }
                }
                Ok(Value::Bool(false))
            }
            ExpressionKind::Cond {
                clauses,
                alternative,
            } => {
                for clause in clauses {
                    if self
                        .evaluate(&clause.condition, environment, depth)?
                        .truthy()
                    {
                        return self.evaluate(&clause.expression, environment, depth);
                    }
                }
                self.evaluate(alternative, environment, depth)
            }
            ExpressionKind::Match { value, arms } => {
                let value = self.evaluate(value, environment, depth)?;
                for arm in arms {
                    if let Some(bindings) =
                        bindings_for_pattern(&arm.pattern, &value, &mut self.budget)?
                    {
                        let local = self.new_environment(Some(environment));
                        for (name, value) in bindings {
                            self.define(local, name, value);
                        }
                        return self.evaluate(&arm.expression, local, depth);
                    }
                }
                Err(Diagnostic::simple(
                    "RUNTIME_MATCH_NOT_EXHAUSTIVE",
                    "match did not select an arm",
                ))
            }
            ExpressionKind::Let { bindings, body } => {
                let local = self.new_environment(Some(environment));
                for binding in bindings {
                    let value = self.evaluate(&binding.expression, local, depth)?;
                    self.define(local, binding.name.clone(), value);
                }
                self.evaluate(body, local, depth)
            }
            ExpressionKind::Function { parameters, body } => {
                let identifier = self.closures.len();
                let body = self.interpreted_body(body);
                self.closures.push(Closure {
                    parameters: parameters.clone(),
                    body: ClosureBody::Interpreted(body),
                    environment,
                });
                Ok(Value::Closure(identifier))
            }
            ExpressionKind::Do(expressions) => {
                let mut result = Value::Nil;
                for item in expressions {
                    result = self.evaluate(item, environment, depth)?;
                }
                Ok(result)
            }
            ExpressionKind::Call { callee, arguments } => {
                let callable = self.evaluate(callee, environment, depth)?;
                let values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument, environment, depth))
                    .collect::<YanshuResult<Vec<_>>>()?;
                self.apply(callable, values, depth + 1)
            }
        }
    }

    fn apply(
        &mut self,
        callable: Value,
        arguments: Vec<Value>,
        depth: usize,
    ) -> YanshuResult<Value> {
        self.budget.consume(1)?;
        self.budget.check_depth(depth)?;
        self.charge_runtime_value(&callable)?;
        for argument in &arguments {
            self.charge_runtime_value(argument)?;
        }
        let result = match callable {
            Value::Closure(identifier) => {
                let closure = self.closures.get(identifier).cloned().ok_or_else(|| {
                    Diagnostic::simple(
                        "RUNTIME_UNKNOWN_CLOSURE",
                        "interpreter received an unknown closure identifier",
                    )
                })?;
                if closure.parameters.len() != arguments.len() {
                    return Err(arity_error(
                        "function",
                        closure.parameters.len(),
                        Some(closure.parameters.len()),
                        arguments.len(),
                    ));
                }
                let call_environment = self.new_environment(Some(closure.environment));
                for (parameter, argument) in closure.parameters.into_iter().zip(arguments) {
                    self.define(call_environment, parameter, argument);
                }
                match closure.body {
                    ClosureBody::Interpreted(body) => self.evaluate(&body, call_environment, depth),
                    ClosureBody::Bytecode(block) => {
                        let code = self.compiled_block(block)?;
                        self.execute_block(&code, call_environment, depth)
                    }
                }
            }
            Value::Primitive(primitive) => {
                check_arity(primitive, arguments.len())?;
                self.apply_primitive(primitive, arguments, depth)
            }
            Value::LibraryFunction {
                name,
                library,
                version,
                operation,
                arity,
            } => {
                if arity != arguments.len() {
                    return Err(arity_error(&name, arity, Some(arity), arguments.len()));
                }
                self.apply_library(&library, version, &operation, &arguments)
            }
            Value::Constructor {
                type_name,
                variant,
                arity,
            } => {
                if arity != arguments.len() {
                    return Err(arity_error(&variant, arity, Some(arity), arguments.len()));
                }
                Ok(Value::Variant {
                    type_name,
                    variant,
                    fields: arguments,
                })
            }
            value => Err(Diagnostic::new(
                "RUNTIME_NOT_CALLABLE",
                "attempted to call a non-callable value",
                json!({ "kind": value.kind() }),
            )),
        }?;
        self.charge_runtime_value(&result)?;
        Ok(result)
    }

    fn charge_runtime_value(&mut self, value: &Value) -> YanshuResult<()> {
        self.budget
            .consume(measure_runtime_value(value)?.fuel_cost())
    }

    fn charge_datum(&mut self, datum: &yanshu_syntax::Datum) -> YanshuResult<()> {
        self.budget.consume(measure_datum(datum)?.fuel_cost())
    }

    fn preflight_integer_product(&mut self, left: &BigInt, right: &BigInt) -> YanshuResult<()> {
        if left.is_zero() || right.is_zero() {
            return Ok(());
        }
        let minimum_result_bits = left.bits().saturating_add(right.bits()).saturating_sub(1);
        if minimum_result_bits > MAXIMUM_INTEGER_BITS {
            return Err(runtime_value_limit(
                "integerBits",
                usize::try_from(MAXIMUM_INTEGER_BITS).unwrap_or(usize::MAX),
                usize::try_from(minimum_result_bits).unwrap_or(usize::MAX),
            ));
        }
        self.charge_integer_quadratic(left, right)
    }

    fn charge_integer_quadratic(&mut self, left: &BigInt, right: &BigInt) -> YanshuResult<()> {
        let left_limbs = left.bits().div_ceil(64).max(1);
        let right_limbs = right.bits().div_ceil(64).max(1);
        self.budget.consume(left_limbs.saturating_mul(right_limbs))
    }

    fn charge_integer_conversion(&mut self, value: &BigInt) -> YanshuResult<()> {
        let limbs = value.bits().div_ceil(64).max(1);
        self.budget.consume(limbs.saturating_mul(limbs))
    }

    fn install_base_environment(&mut self, program: &Program, environment: usize) {
        use PrimitiveOperation as Operation;
        let primitives = [
            primitive("+", 0, None, Operation::Add),
            primitive("*", 0, None, Operation::Multiply),
            primitive("-", 1, None, Operation::Subtract),
            primitive("quotient", 2, Some(2), Operation::Quotient),
            primitive("remainder", 2, Some(2), Operation::Remainder),
            primitive("=", 2, Some(2), Operation::Equal),
            primitive("<", 2, Some(2), Operation::Less),
            primitive("<=", 2, Some(2), Operation::LessEqual),
            primitive(">", 2, Some(2), Operation::Greater),
            primitive(">=", 2, Some(2), Operation::GreaterEqual),
            primitive("not", 1, Some(1), Operation::Not),
            primitive("integer?", 1, Some(1), Operation::IsInteger),
            primitive("boolean?", 1, Some(1), Operation::IsBoolean),
            primitive("string?", 1, Some(1), Operation::IsString),
            primitive("list?", 1, Some(1), Operation::IsList),
            primitive("map?", 1, Some(1), Operation::IsMap),
            primitive("string-append", 0, None, Operation::StringAppend),
            primitive("list", 0, None, Operation::List),
            primitive("empty?", 1, Some(1), Operation::IsEmpty),
            primitive("length", 1, Some(1), Operation::Length),
            primitive("first", 1, Some(1), Operation::First),
            primitive("rest", 1, Some(1), Operation::Rest),
            primitive("map", 0, None, Operation::Map),
            primitive("get", 2, Some(2), Operation::Get),
            primitive("get-or", 3, Some(3), Operation::GetOr),
            primitive("has-key?", 2, Some(2), Operation::HasKey),
            primitive("assoc", 3, Some(3), Operation::Assoc),
            primitive("validate", 2, Some(2), Operation::Validate),
            primitive("ok", 1, Some(1), Operation::Ok),
            primitive("err", 1, Some(1), Operation::Err),
            primitive("ok?", 1, Some(1), Operation::IsOk),
            primitive("err?", 1, Some(1), Operation::IsErr),
            primitive("result-value", 1, Some(1), Operation::ResultValue),
            primitive("unwrap", 1, Some(1), Operation::Unwrap),
            primitive("api-response", 2, Some(2), Operation::ApiResponse),
            primitive("api-error", 3, Some(4), Operation::ApiError),
        ];
        for item in primitives {
            self.define(environment, item.name.to_owned(), Value::Primitive(item));
        }
        if program.version >= BigInt::from(2_u8) {
            let primitives = [
                primitive("number->string", 1, Some(1), Operation::NumberToString),
                primitive("validate-report", 2, Some(2), Operation::ValidateReport),
                primitive("list-map", 2, Some(2), Operation::ListMap),
                primitive("list-filter", 2, Some(2), Operation::ListFilter),
                primitive("list-fold", 3, Some(3), Operation::ListFold),
                primitive("sum", 1, Some(1), Operation::Sum),
                primitive("checked-quotient", 2, Some(2), Operation::CheckedQuotient),
                primitive("checked-remainder", 2, Some(2), Operation::CheckedRemainder),
            ];
            for item in primitives {
                self.define(environment, item.name.to_owned(), Value::Primitive(item));
            }
        }
    }

    fn install_libraries(&mut self, program: &Program, environment: usize) -> YanshuResult<()> {
        for requirement in &program.libraries {
            let contract = self
                .libraries
                .require(&requirement.name, requirement.version)?;
            for operation in contract.operations {
                let name = format!("{}/{}", requirement.name, operation.name);
                self.define(
                    environment,
                    name.clone(),
                    Value::LibraryFunction {
                        name,
                        library: requirement.name.clone(),
                        version: requirement.version,
                        operation: operation.name.to_owned(),
                        arity: operation.parameters.len(),
                    },
                );
            }
        }
        Ok(())
    }

    fn install_capabilities(&mut self, program: &Program, environment: usize) -> YanshuResult<()> {
        use PrimitiveOperation as Operation;
        for capability in &program.capabilities {
            match capability.as_str() {
                "log" => {
                    self.require_capability("log")?;
                    let item = primitive("log", 1, Some(1), Operation::Log);
                    self.define(environment, item.name.to_owned(), Value::Primitive(item));
                }
                "clock" => {
                    self.require_capability("clock")?;
                    let item = primitive("now-ms", 0, Some(0), Operation::NowMilliseconds);
                    self.define(environment, item.name.to_owned(), Value::Primitive(item));
                }
                "kv" => {
                    self.require_capability("kv")?;
                    let operations = [
                        primitive("kv-get", 2, Some(2), Operation::KvGet),
                        primitive("kv-put", 2, Some(2), Operation::KvPut),
                        primitive("kv-delete", 1, Some(1), Operation::KvDelete),
                        primitive("kv-list", 1, Some(1), Operation::KvList),
                    ];
                    for item in operations {
                        self.define(environment, item.name.to_owned(), Value::Primitive(item));
                    }
                }
                _ => {
                    return Err(Diagnostic::new(
                        "RUNTIME_CAPABILITY_UNAVAILABLE",
                        "host did not provide a declared capability",
                        json!({ "capability": capability }),
                    ));
                }
            }
        }
        Ok(())
    }

    fn require_capability(&self, capability: &str) -> YanshuResult<()> {
        if self
            .host
            .as_deref()
            .is_some_and(|host| host.supports(capability))
        {
            Ok(())
        } else {
            Err(Diagnostic::new(
                "RUNTIME_CAPABILITY_UNAVAILABLE",
                "host did not provide a declared capability",
                json!({ "capability": capability }),
            ))
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_primitive(
        &mut self,
        primitive: Primitive,
        arguments: Vec<Value>,
        depth: usize,
    ) -> YanshuResult<Value> {
        use PrimitiveOperation as Operation;
        match primitive.operation {
            Operation::Add => {
                let mut result = BigInt::zero();
                for argument in &arguments {
                    result += expect_integer(primitive.name, argument)?;
                }
                Ok(Value::Int(result))
            }
            Operation::Multiply => {
                let mut result = BigInt::one();
                for argument in &arguments {
                    let factor = expect_integer(primitive.name, argument)?;
                    self.preflight_integer_product(&result, factor)?;
                    result *= factor;
                }
                Ok(Value::Int(result))
            }
            Operation::Subtract => {
                let first = expect_integer(primitive.name, &arguments[0])?.clone();
                if arguments.len() == 1 {
                    Ok(Value::Int(-first))
                } else {
                    let mut result = first;
                    for argument in &arguments[1..] {
                        result -= expect_integer(primitive.name, argument)?;
                    }
                    Ok(Value::Int(result))
                }
            }
            Operation::Quotient
            | Operation::Remainder
            | Operation::CheckedQuotient
            | Operation::CheckedRemainder => {
                let numerator = expect_integer(primitive.name, &arguments[0])?;
                let denominator = expect_integer(primitive.name, &arguments[1])?;
                if denominator.is_zero() {
                    if matches!(
                        primitive.operation,
                        Operation::CheckedQuotient | Operation::CheckedRemainder
                    ) {
                        return Ok(Value::Err(Box::new(string_map([
                            ("code", Value::String("DIVIDE_BY_ZERO".to_owned())),
                            ("operation", Value::String(primitive.name.to_owned())),
                        ]))));
                    }
                    return Err(Diagnostic::simple(
                        "RUNTIME_DIVIDE_BY_ZERO",
                        if matches!(
                            primitive.operation,
                            Operation::Quotient | Operation::CheckedQuotient
                        ) {
                            "quotient denominator cannot be zero"
                        } else {
                            "remainder denominator cannot be zero"
                        },
                    ));
                }
                self.charge_integer_quadratic(numerator, denominator)?;
                let value = Value::Int(
                    if matches!(
                        primitive.operation,
                        Operation::Quotient | Operation::CheckedQuotient
                    ) {
                        numerator / denominator
                    } else {
                        numerator % denominator
                    },
                );
                if matches!(
                    primitive.operation,
                    Operation::CheckedQuotient | Operation::CheckedRemainder
                ) {
                    Ok(Value::Ok(Box::new(value)))
                } else {
                    Ok(value)
                }
            }
            Operation::Equal => Ok(Value::Bool(arguments[0] == arguments[1])),
            Operation::Less
            | Operation::LessEqual
            | Operation::Greater
            | Operation::GreaterEqual => {
                let left = expect_integer(primitive.name, &arguments[0])?;
                let right = expect_integer(primitive.name, &arguments[1])?;
                Ok(Value::Bool(match primitive.operation {
                    Operation::Less => left < right,
                    Operation::LessEqual => left <= right,
                    Operation::Greater => left > right,
                    Operation::GreaterEqual => left >= right,
                    _ => false,
                }))
            }
            Operation::Not => Ok(Value::Bool(!arguments[0].truthy())),
            Operation::IsInteger => Ok(Value::Bool(matches!(arguments[0], Value::Int(_)))),
            Operation::IsBoolean => Ok(Value::Bool(matches!(arguments[0], Value::Bool(_)))),
            Operation::IsString => Ok(Value::Bool(matches!(arguments[0], Value::String(_)))),
            Operation::IsList => Ok(Value::Bool(matches!(
                arguments[0],
                Value::Nil | Value::List(_)
            ))),
            Operation::IsMap => Ok(Value::Bool(matches!(arguments[0], Value::Map(_)))),
            Operation::StringAppend => {
                let total_bytes = arguments.iter().try_fold(0_usize, |total, argument| {
                    total
                        .checked_add(expect_string(primitive.name, argument)?.len())
                        .ok_or_else(|| {
                            runtime_value_limit("scalarBytes", MAXIMUM_VALUE_BYTES, usize::MAX)
                        })
                })?;
                if total_bytes > MAXIMUM_VALUE_BYTES {
                    return Err(runtime_value_limit(
                        "scalarBytes",
                        MAXIMUM_VALUE_BYTES,
                        total_bytes,
                    ));
                }
                self.budget.consume(bytes_fuel(total_bytes))?;
                let mut result = String::with_capacity(total_bytes);
                for argument in &arguments {
                    result.push_str(expect_string(primitive.name, argument)?);
                }
                Ok(Value::String(result))
            }
            Operation::NumberToString => {
                let integer = expect_integer(primitive.name, &arguments[0])?;
                self.charge_integer_conversion(integer)?;
                Ok(Value::String(integer.to_string()))
            }
            Operation::List => Ok(list_value(arguments)),
            Operation::ListMap => {
                let callable = arguments[0].clone();
                let values = expect_list(primitive.name, &arguments[1])?.to_vec();
                let mut mapped = Vec::with_capacity(values.len());
                for value in values {
                    self.budget.consume(1)?;
                    mapped.push(self.apply(callable.clone(), vec![value], depth + 1)?);
                }
                Ok(list_value(mapped))
            }
            Operation::ListFilter => {
                let callable = arguments[0].clone();
                let values = expect_list(primitive.name, &arguments[1])?.to_vec();
                let mut filtered = Vec::with_capacity(values.len());
                for value in values {
                    self.budget.consume(1)?;
                    let keep = self.apply(callable.clone(), vec![value.clone()], depth + 1)?;
                    if keep.truthy() {
                        filtered.push(value);
                    }
                }
                Ok(list_value(filtered))
            }
            Operation::ListFold => {
                let callable = arguments[0].clone();
                let mut accumulator = arguments[1].clone();
                let values = expect_list(primitive.name, &arguments[2])?.to_vec();
                for value in values {
                    self.budget.consume(1)?;
                    accumulator =
                        self.apply(callable.clone(), vec![accumulator, value], depth + 1)?;
                }
                Ok(accumulator)
            }
            Operation::Sum => {
                let mut total = BigInt::zero();
                for value in expect_list(primitive.name, &arguments[0])? {
                    self.budget.consume(1)?;
                    total += expect_integer(primitive.name, value)?;
                }
                Ok(Value::Int(total))
            }
            Operation::IsEmpty => Ok(Value::Bool(
                expect_list(primitive.name, &arguments[0])?.is_empty(),
            )),
            Operation::Length => Ok(Value::Int(
                expect_list(primitive.name, &arguments[0])?.len().into(),
            )),
            Operation::First => {
                let values = expect_list(primitive.name, &arguments[0])?;
                values.first().cloned().ok_or_else(|| {
                    Diagnostic::simple(
                        "RUNTIME_EMPTY_COLLECTION",
                        "first cannot read an empty list",
                    )
                })
            }
            Operation::Rest => {
                let values = expect_list(primitive.name, &arguments[0])?;
                if values.is_empty() {
                    return Err(Diagnostic::simple(
                        "RUNTIME_EMPTY_COLLECTION",
                        "rest cannot read an empty list",
                    ));
                }
                Ok(list_value(values[1..].to_vec()))
            }
            Operation::Map => build_map(arguments),
            Operation::Get => {
                let mapping = expect_map(primitive.name, &arguments[0])?;
                let key = map_key(&arguments[1]);
                key.and_then(|value| mapping.get(&value))
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(
                            "RUNTIME_MISSING_KEY",
                            "map does not contain the requested key",
                            json!({ "key": arguments[1].display() }),
                        )
                    })
            }
            Operation::GetOr => {
                let mapping = expect_map(primitive.name, &arguments[0])?;
                Ok(map_key(&arguments[1])
                    .and_then(|key| mapping.get(&key))
                    .cloned()
                    .unwrap_or_else(|| arguments[2].clone()))
            }
            Operation::HasKey => {
                let mapping = expect_map(primitive.name, &arguments[0])?;
                Ok(Value::Bool(
                    map_key(&arguments[1]).is_some_and(|key| mapping.contains_key(&key)),
                ))
            }
            Operation::Assoc => {
                let mut mapping = expect_map(primitive.name, &arguments[0])?.clone();
                let key = expect_map_key(primitive.name, &arguments[1])?;
                mapping.insert(key, arguments[2].clone());
                Ok(Value::Map(mapping))
            }
            Operation::Validate => {
                let Value::Schema { specification, .. } = &arguments[0] else {
                    return Err(type_error(primitive.name, "Schema", &arguments[0]));
                };
                let validation = validate_schema(specification, &arguments[1], &mut self.budget)?;
                if validation.valid() {
                    Ok(Value::Ok(Box::new(validation.value)))
                } else {
                    Ok(Value::Err(Box::new(Value::List(validation.issues))))
                }
            }
            Operation::ValidateReport => {
                let Value::Schema { specification, .. } = &arguments[0] else {
                    return Err(type_error(primitive.name, "Schema", &arguments[0]));
                };
                let validation = validate_schema(specification, &arguments[1], &mut self.budget)?;
                Ok(string_map([
                    ("valid", Value::Bool(validation.valid())),
                    ("value", validation.value),
                    ("issues", Value::List(validation.issues)),
                    (
                        "cost",
                        string_map([("fuel", Value::Int(validation.fuel_consumed.into()))]),
                    ),
                ]))
            }
            Operation::Ok => Ok(Value::Ok(Box::new(arguments[0].clone()))),
            Operation::Err => Ok(Value::Err(Box::new(arguments[0].clone()))),
            Operation::IsOk => Ok(Value::Bool(matches!(arguments[0], Value::Ok(_)))),
            Operation::IsErr => Ok(Value::Bool(matches!(arguments[0], Value::Err(_)))),
            Operation::ResultValue => match &arguments[0] {
                Value::Ok(value) | Value::Err(value) => Ok(value.as_ref().clone()),
                value => Err(type_error(primitive.name, "Result", value)),
            },
            Operation::Unwrap => match &arguments[0] {
                Value::Ok(value) => Ok(value.as_ref().clone()),
                Value::Err(value) => Err(Diagnostic::new(
                    "RUNTIME_UNWRAP_ERROR",
                    "cannot unwrap an Err value",
                    json!({ "value": value.to_json()? }),
                )),
                value => Err(type_error(primitive.name, "Result", value)),
            },
            Operation::ApiResponse => {
                let status = expect_status(primitive.name, &arguments[0], 100)?;
                Ok(string_map([
                    ("status", Value::Int(status)),
                    ("headers", Value::Map(BTreeMap::new())),
                    ("body", arguments[1].clone()),
                ]))
            }
            Operation::ApiError => build_api_error(&arguments),
            Operation::Log => self.invoke_capability("log", &arguments),
            Operation::NowMilliseconds
            | Operation::KvGet
            | Operation::KvPut
            | Operation::KvDelete
            | Operation::KvList => self.invoke_capability(primitive.name, &arguments),
            Operation::TextLength
            | Operation::TextStartsWith
            | Operation::TextEndsWith
            | Operation::TextContains
            | Operation::TextReplace => Err(Diagnostic::simple(
                "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                "library primitive bypassed the registered backend boundary",
            )),
        }
    }

    fn invoke_capability(&mut self, operation: &str, arguments: &[Value]) -> YanshuResult<Value> {
        let result = self.host.as_deref_mut().map_or_else(
            || {
                Err(Diagnostic::new(
                    "RUNTIME_CAPABILITY_UNAVAILABLE",
                    "host did not provide a declared capability",
                    json!({ "capability": operation }),
                ))
            },
            |host| host.invoke(operation, arguments),
        )?;
        validate_capability_result(operation, &result)?;
        self.budget
            .consume(measure_portable_value(&result)?.fuel_cost())?;
        Ok(result)
    }

    fn apply_library(
        &mut self,
        library: &str,
        version: u16,
        operation: &str,
        arguments: &[Value],
    ) -> YanshuResult<Value> {
        let portable = arguments
            .iter()
            .map(value_to_library)
            .collect::<YanshuResult<Vec<_>>>()?;
        let fuel = self
            .libraries
            .call_fuel(library, version, operation, &portable)?;
        self.budget.consume(fuel)?;
        let invocation = self
            .libraries
            .invoke(library, version, operation, &portable)?;
        debug_assert_eq!(fuel, invocation.fuel);
        let result = library_to_value(&invocation.value);
        self.normalize_library_result(&result, 0, &mut 0)
    }

    fn normalize_library_result(
        &mut self,
        value: &Value,
        depth: usize,
        node_count: &mut usize,
    ) -> YanshuResult<Value> {
        *node_count += 1;
        self.budget.consume(1)?;
        if *node_count > 10_000 {
            return Err(Diagnostic::new(
                "RUNTIME_LIBRARY_INVALID_RESULT",
                "library backend result exceeds the node limit",
                json!({ "maximum": 10_000 }),
            ));
        }
        if depth > 64 {
            return Err(Diagnostic::new(
                "RUNTIME_LIBRARY_INVALID_RESULT",
                "library backend result exceeds the depth limit",
                json!({ "maximum": 64 }),
            ));
        }
        match value {
            Value::String(text) => {
                if text.chars().count() > 1024 * 1024 {
                    return Err(Diagnostic::new(
                        "RUNTIME_LIBRARY_INVALID_RESULT",
                        "library backend result contains an oversized string",
                        json!({ "maximum": 1024 * 1024 }),
                    ));
                }
                let blocks = text.chars().count().saturating_add(63) / 64;
                self.budget
                    .consume(u64::try_from(blocks).unwrap_or(u64::MAX))?;
                Ok(value.clone())
            }
            Value::List(items) => Ok(Value::List(
                items
                    .iter()
                    .map(|item| self.normalize_library_result(item, depth + 1, node_count))
                    .collect::<YanshuResult<Vec<_>>>()?,
            )),
            Value::Map(items) => {
                let mut normalized = BTreeMap::new();
                for (key, item) in items {
                    normalized.insert(
                        key.clone(),
                        self.normalize_library_result(item, depth + 1, node_count)?,
                    );
                }
                Ok(Value::Map(normalized))
            }
            Value::Ok(item) => Ok(Value::Ok(Box::new(self.normalize_library_result(
                item,
                depth + 1,
                node_count,
            )?))),
            Value::Err(item) => Ok(Value::Err(Box::new(self.normalize_library_result(
                item,
                depth + 1,
                node_count,
            )?))),
            Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Symbol(_) => Ok(value.clone()),
            _ => Err(Diagnostic::new(
                "RUNTIME_LIBRARY_INVALID_RESULT",
                "library backend result is not portable guest data",
                json!({ "kind": value.kind() }),
            )),
        }
    }
}

fn value_to_library(value: &Value) -> YanshuResult<LibraryValue> {
    match value {
        Value::Nil => Ok(LibraryValue::Nil),
        Value::Bool(value) => Ok(LibraryValue::Bool(*value)),
        Value::Int(value) => Ok(LibraryValue::Int(value.clone())),
        Value::String(value) => Ok(LibraryValue::String(value.clone())),
        Value::Symbol(value) => Ok(LibraryValue::Symbol(value.clone())),
        Value::List(values) => values
            .iter()
            .map(value_to_library)
            .collect::<YanshuResult<Vec<_>>>()
            .map(LibraryValue::List),
        Value::Map(values) => {
            let mut portable = BTreeMap::new();
            for (key, item) in values {
                portable.insert(
                    match key {
                        MapKey::String(value) => LibraryKey::String(value.clone()),
                        MapKey::Symbol(value) => LibraryKey::Symbol(value.clone()),
                    },
                    value_to_library(item)?,
                );
            }
            Ok(LibraryValue::Map(portable))
        }
        Value::Ok(value) => Ok(LibraryValue::Ok(Box::new(value_to_library(value)?))),
        Value::Err(value) => Ok(LibraryValue::Err(Box::new(value_to_library(value)?))),
        Value::Variant {
            type_name,
            variant,
            fields,
        } => Ok(LibraryValue::Variant {
            type_name: type_name.clone(),
            variant: variant.clone(),
            fields: fields
                .iter()
                .map(value_to_library)
                .collect::<YanshuResult<Vec<_>>>()?,
        }),
        _ => Err(Diagnostic::new(
            "RUNTIME_LIBRARY_INVALID_ARGUMENT",
            "library boundary accepts only immutable portable guest data",
            json!({ "kind": value.kind() }),
        )),
    }
}

fn library_to_value(value: &LibraryValue) -> Value {
    match value {
        LibraryValue::Nil => Value::Nil,
        LibraryValue::Bool(value) => Value::Bool(*value),
        LibraryValue::Int(value) => Value::Int(value.clone()),
        LibraryValue::String(value) => Value::String(value.clone()),
        LibraryValue::Symbol(value) => Value::Symbol(value.clone()),
        LibraryValue::List(values) => Value::List(values.iter().map(library_to_value).collect()),
        LibraryValue::Map(values) => Value::Map(
            values
                .iter()
                .map(|(key, item)| {
                    (
                        match key {
                            LibraryKey::String(value) => MapKey::String(value.clone()),
                            LibraryKey::Symbol(value) => MapKey::Symbol(value.clone()),
                        },
                        library_to_value(item),
                    )
                })
                .collect(),
        ),
        LibraryValue::Ok(value) => Value::Ok(Box::new(library_to_value(value))),
        LibraryValue::Err(value) => Value::Err(Box::new(library_to_value(value))),
        LibraryValue::Variant {
            type_name,
            variant,
            fields,
        } => Value::Variant {
            type_name: type_name.clone(),
            variant: variant.clone(),
            fields: fields.iter().map(library_to_value).collect(),
        },
    }
}

fn primitive(
    name: &'static str,
    minimum_arity: usize,
    maximum_arity: Option<usize>,
    operation: PrimitiveOperation,
) -> Primitive {
    Primitive {
        name,
        minimum_arity,
        maximum_arity,
        operation,
    }
}

fn check_arity(primitive: Primitive, actual: usize) -> YanshuResult<()> {
    if actual < primitive.minimum_arity
        || primitive
            .maximum_arity
            .is_some_and(|maximum| actual > maximum)
    {
        return Err(arity_error(
            primitive.name,
            primitive.minimum_arity,
            primitive.maximum_arity,
            actual,
        ));
    }
    Ok(())
}

fn bytes_fuel(bytes: usize) -> u64 {
    u64::try_from(bytes.div_ceil(64)).unwrap_or(u64::MAX)
}

fn runtime_value_limit(kind: &str, maximum: usize, actual: usize) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_VALUE_LIMIT",
        "guest value exceeds a structural resource limit",
        json!({
            "kind": kind,
            "maximum": maximum,
            "actual": actual,
        }),
    )
}

fn arity_error(name: &str, minimum: usize, maximum: Option<usize>, actual: usize) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_ARITY",
        "callable received the wrong number of arguments",
        json!({
            "name": name,
            "minimum": minimum,
            "maximum": maximum.map_or(JsonValue::String("unbounded".to_owned()), |value| json!(value)),
            "actual": actual,
        }),
    )
}

fn expect_integer<'value>(operation: &str, value: &'value Value) -> YanshuResult<&'value BigInt> {
    match value {
        Value::Int(integer) => Ok(integer),
        _ => Err(type_error(operation, "Int", value)),
    }
}

fn expect_string<'value>(operation: &str, value: &'value Value) -> YanshuResult<&'value str> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(type_error(operation, "String", value)),
    }
}

fn expect_list<'value>(operation: &str, value: &'value Value) -> YanshuResult<&'value [Value]> {
    match value {
        Value::Nil => Ok(&[]),
        Value::List(values) => Ok(values),
        _ => Err(type_error(operation, "List", value)),
    }
}

fn expect_map<'value>(
    operation: &str,
    value: &'value Value,
) -> YanshuResult<&'value BTreeMap<MapKey, Value>> {
    match value {
        Value::Map(mapping) => Ok(mapping),
        _ => Err(type_error(operation, "Map", value)),
    }
}

fn expect_map_key(operation: &str, value: &Value) -> YanshuResult<MapKey> {
    map_key(value).ok_or_else(|| type_error(operation, "String or Symbol key", value))
}

fn map_key(value: &Value) -> Option<MapKey> {
    match value {
        Value::String(text) => Some(MapKey::String(text.clone())),
        Value::Symbol(symbol) => Some(MapKey::Symbol(symbol.clone())),
        _ => None,
    }
}

fn type_error(operation: &str, expected: &str, value: &Value) -> Diagnostic {
    Diagnostic::new(
        "RUNTIME_TYPE",
        "primitive received a value of the wrong type",
        json!({
            "operation": operation,
            "expected": expected,
            "actual": value.kind(),
        }),
    )
}

fn list_value(values: Vec<Value>) -> Value {
    if values.is_empty() {
        Value::Nil
    } else {
        Value::List(values)
    }
}

fn build_map(arguments: Vec<Value>) -> YanshuResult<Value> {
    if !arguments.len().is_multiple_of(2) {
        return Err(Diagnostic::simple(
            "RUNTIME_MAP_ARITY",
            "map expects alternating key and value arguments",
        ));
    }
    let mut mapping = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        let key = expect_map_key("map", &pair[0])?;
        mapping.insert(key, pair[1].clone());
    }
    Ok(Value::Map(mapping))
}

fn expect_status(operation: &str, value: &Value, minimum: i64) -> YanshuResult<BigInt> {
    let actual = match value {
        Value::Int(integer) => {
            let minimum_value = BigInt::from(minimum);
            if integer >= &minimum_value && integer <= &BigInt::from(599) {
                return Ok(integer.clone());
            }
            bigint_json(integer)
        }
        _ => JsonValue::String(value.kind().to_owned()),
    };
    Err(Diagnostic::new(
        "RUNTIME_INVALID_HTTP_STATUS",
        "HTTP response status is outside the allowed range",
        json!({ "operation": operation, "minimum": minimum, "actual": actual }),
    ))
}

fn build_api_error(arguments: &[Value]) -> YanshuResult<Value> {
    let status = expect_status("api-error", &arguments[0], 400)?;
    let Value::String(code) = &arguments[1] else {
        return Err(Diagnostic::simple(
            "RUNTIME_INVALID_API_ERROR",
            "api-error code must be a bounded uppercase identifier",
        ));
    };
    if code.is_empty() || code.chars().count() > 128 || !valid_api_error_code(code) {
        return Err(Diagnostic::simple(
            "RUNTIME_INVALID_API_ERROR",
            "api-error code must be a bounded uppercase identifier",
        ));
    }
    let Value::String(message) = &arguments[2] else {
        return Err(Diagnostic::simple(
            "RUNTIME_INVALID_API_ERROR",
            "api-error message must be a non-empty bounded string",
        ));
    };
    if message.is_empty() || message.chars().count() > 512 {
        return Err(Diagnostic::simple(
            "RUNTIME_INVALID_API_ERROR",
            "api-error message must be a non-empty bounded string",
        ));
    }
    let details = arguments
        .get(3)
        .cloned()
        .unwrap_or_else(|| Value::Map(BTreeMap::new()));
    Ok(string_map([
        ("status", Value::Int(status)),
        ("headers", Value::Map(BTreeMap::new())),
        (
            "body",
            string_map([(
                "error",
                string_map([
                    ("code", Value::String(code.clone())),
                    ("message", Value::String(message.clone())),
                    ("details", details),
                ]),
            )]),
        ),
    ]))
}

fn valid_api_error_code(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

fn string_map<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (MapKey::String(key.to_owned()), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use num_bigint::BigInt;
    use serde_json::json;
    use yanshu_compiler::compile_bytecode;
    use yanshu_diagnostic::{Diagnostic, YanshuResult};
    use yanshu_library::{BackendDescriptor, LibraryBackend, LibraryRegistry, LibraryValue};
    use yanshu_syntax::load_program_source;

    use super::{
        CapabilityHost, ExecutionOptions, MapKey, Value, execute_compiled_export,
        execute_compiled_export_report, execute_export, execute_export_with_host,
        execute_export_with_libraries, json_to_value,
    };

    const CORE: &str = include_str!("../../../../conformance/v1/programs/core.yan");
    const SCHEMA: &str = include_str!("../../../../conformance/v1/programs/schema.yan");
    const LIBRARY: &str = include_str!("../../../../conformance/v1/programs/library.yan");

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

    struct OversizedCapabilityHost;

    impl CapabilityHost for OversizedCapabilityHost {
        fn supports(&self, capability: &str) -> bool {
            capability == "kv" || capability == "kv-get"
        }

        fn invoke(&mut self, operation: &str, _arguments: &[Value]) -> YanshuResult<Value> {
            assert_eq!(operation, "kv-get");
            Ok(Value::String("x".repeat(1024 * 1024 + 1)))
        }
    }

    #[test]
    fn guest_values_are_bounded_before_expensive_runtime_work() {
        let oversized_json = json!("x".repeat(1024 * 1024 + 1));
        assert_eq!(
            require_error(json_to_value(&oversized_json)).code,
            "RUNTIME_VALUE_LIMIT"
        );

        let mut nested_json = json!(null);
        for _ in 0..=65 {
            nested_json = json!([nested_json]);
        }
        assert_eq!(
            require_error(json_to_value(&nested_json)).code,
            "RUNTIME_VALUE_LIMIT"
        );

        let string_program = require(load_program_source(
            r#"(program
                (name bounded-strings)
                (version 4)
                (signature run (fn (string string) string))
                (def run (fn (left right) (string-append left right)))
                (export run))"#,
        ));
        let string_limit = require_error(execute_export(
            &string_program,
            "run",
            vec![
                Value::String("a".repeat(600 * 1024)),
                Value::String("b".repeat(600 * 1024)),
            ],
            ExecutionOptions {
                fuel: 100_000,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(string_limit.code, "RUNTIME_VALUE_LIMIT");

        let integer_program = require(load_program_source(
            r#"(program
                (name bounded-integers)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value) (* value value)))
                (export run))"#,
        ));
        let large_integer = BigInt::from(1_u8) << 32_768_usize;
        let integer_limit = require_error(execute_export(
            &integer_program,
            "run",
            vec![Value::Int(large_integer)],
            ExecutionOptions {
                fuel: 10_000,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(integer_limit.code, "RUNTIME_VALUE_LIMIT");
    }

    #[test]
    fn capability_results_must_obey_the_guest_value_envelope() {
        let program = require(load_program_source(
            r#"(program
                (name bounded-capability)
                (version 4)
                (capabilities kv)
                (signature run (fn () any))
                (def run (fn () (kv-get "space" "key")))
                (export run))"#,
        ));
        let mut host = OversizedCapabilityHost;
        let diagnostic = require_error(execute_export_with_host(
            &program,
            "run",
            Vec::new(),
            ExecutionOptions {
                fuel: 100_000,
                ..ExecutionOptions::default()
            },
            &mut host,
        ));
        assert_eq!(diagnostic.code, "RUNTIME_VALUE_LIMIT");
    }

    #[test]
    fn every_declared_capability_requires_an_explicit_host() {
        let program = require(load_program_source(
            r#"(program
                (name explicit-log-host)
                (version 1)
                (capabilities log)
                (def run (fn (value) (log value)))
                (export run))"#,
        ));
        assert_eq!(
            require_error(execute_export(
                &program,
                "run",
                vec![Value::String("event".to_owned())],
                ExecutionOptions::default(),
            ))
            .code,
            "RUNTIME_CAPABILITY_UNAVAILABLE"
        );
    }

    #[test]
    fn schema_union_cannot_clone_large_values_for_nearly_free() {
        let variants = std::iter::repeat_n("boolean", 8)
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "(program (name bounded-schema) (version 2) \
             (schema candidate (union {variants})) \
             (def run (fn (value) (validate candidate value))) (export run))"
        );
        let program = require(load_program_source(&source));
        let diagnostic = require_error(execute_export(
            &program,
            "run",
            vec![Value::String("x".repeat(64 * 1024))],
            ExecutionOptions {
                fuel: 5_000,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(diagnostic.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn compiled_vm_matches_interpreter_for_typed_control_flow_and_recursion() {
        let program = require(load_program_source(
            r#"(program
                (name compiled-semantics)
                (version 4)
                (data decision
                  (approved (amount integer))
                  (rejected (reason string)))
                (def choose (fn (amount)
                  (if (< amount 0)
                      (rejected "negative")
                      (approved amount))))
                (signature run (fn (integer) map))
                (def run (fn (amount)
                  (match (choose amount)
                    ((approved value) (map "status" "approved" "amount" value))
                    ((rejected reason) (map "status" "rejected" "reason" reason))
                    (_ (map "status" "invalid")))))
                (signature factorial (fn (integer) integer))
                (def factorial (fn (value)
                  (if (= value 0)
                      1
                      (* value (factorial (- value 1))))))
                (export run factorial))"#,
        ));
        let artifact = require(compile_bytecode(&program));
        for amount in [-4, 0, 1_200] {
            let arguments = vec![Value::Int(amount.into())];
            let interpreted = require(execute_export(
                &program,
                "run",
                arguments.clone(),
                ExecutionOptions::default(),
            ));
            let compiled = require(execute_compiled_export(
                &artifact,
                "run",
                arguments,
                ExecutionOptions::default(),
            ));
            assert_eq!(compiled, interpreted);
        }
        assert_eq!(
            require(execute_compiled_export(
                &artifact,
                "factorial",
                vec![Value::Int(8.into())],
                ExecutionOptions::default(),
            )),
            Value::Int(40_320.into())
        );
        let exhausted = require_error(execute_compiled_export(
            &artifact,
            "factorial",
            vec![Value::Int(8.into())],
            ExecutionOptions {
                fuel: 5,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn compiled_vm_reports_deterministic_path_and_collection_fuel() {
        let program = require(load_program_source(
            r#"(program
                (name compiled-cost)
                (version 4)
                (signature total (fn ((list integer)) integer))
                (def total (fn (values) (sum values)))
                (export total))"#,
        ));
        let artifact = require(compile_bytecode(&program));
        let short = require(execute_compiled_export_report(
            &artifact,
            "total",
            vec![Value::List(vec![Value::Int(1.into())])],
            ExecutionOptions::default(),
        ));
        let long_arguments = vec![Value::List(
            (1..=100).map(|value| Value::Int(value.into())).collect(),
        )];
        let long = require(execute_compiled_export_report(
            &artifact,
            "total",
            long_arguments.clone(),
            ExecutionOptions::default(),
        ));
        let repeated = require(execute_compiled_export_report(
            &artifact,
            "total",
            long_arguments.clone(),
            ExecutionOptions::default(),
        ));
        assert_eq!(short.value, Value::Int(1.into()));
        assert_eq!(long.value, Value::Int(5_050.into()));
        assert!(long.fuel_consumed > short.fuel_consumed);
        assert_eq!(long, repeated);
        assert_eq!(long.fuel_limit, long.fuel_consumed + long.fuel_remaining);

        let exact = ExecutionOptions {
            fuel: long.fuel_consumed,
            ..ExecutionOptions::default()
        };
        assert_eq!(
            require(execute_export(
                &program,
                "total",
                long_arguments.clone(),
                exact,
            )),
            long.value
        );
        assert_eq!(
            require(execute_compiled_export(
                &artifact,
                "total",
                long_arguments.clone(),
                exact
            )),
            long.value
        );
        let below = ExecutionOptions {
            fuel: long.fuel_consumed.saturating_sub(1),
            ..ExecutionOptions::default()
        };
        assert_eq!(
            require_error(execute_export(
                &program,
                "total",
                long_arguments.clone(),
                below
            ))
            .code,
            "RUNTIME_FUEL_EXHAUSTED"
        );
        assert_eq!(
            require_error(execute_compiled_export(
                &artifact,
                "total",
                long_arguments,
                below
            ))
            .code,
            "RUNTIME_FUEL_EXHAUSTED"
        );
    }

    #[test]
    fn compiled_vm_preserves_short_circuit_cond_and_sequential_let() {
        let program = require(load_program_source(
            r#"(program
                (name compiled-forms)
                (version 4)
                (signature run (fn (integer integer) integer))
                (def run (fn (left right)
                  (let ((selected (and left right))
                        (guard (or (> selected 0) (= selected 0)))
                        (bump (fn (value) (+ value 1))))
                    (cond
                      ((and (= left 0) (= right 99)) (bump 9))
                      (guard (bump selected))
                      (else (- selected))))))
                (export run))"#,
        ));
        let artifact = require(compile_bytecode(&program));
        for (left, right) in [(-1, -3), (0, 99), (2, 7)] {
            let arguments = vec![Value::Int(left.into()), Value::Int(right.into())];
            let interpreted = require(execute_export(
                &program,
                "run",
                arguments.clone(),
                ExecutionOptions::default(),
            ));
            let compiled = require(execute_compiled_export(
                &artifact,
                "run",
                arguments,
                ExecutionOptions::default(),
            ));
            assert_eq!(compiled, interpreted);
        }
    }

    #[test]
    fn evaluates_recursion_sequential_let_truthiness_and_bigints() {
        let program = require(load_program_source(CORE));
        assert_eq!(
            require(execute_export(
                &program,
                "factorial",
                vec![Value::Int(5.into())],
                ExecutionOptions::default(),
            )),
            Value::Int(120.into())
        );
        assert_eq!(
            require(execute_export(
                &program,
                "sequential-let",
                vec![Value::Int(5.into())],
                ExecutionOptions::default(),
            )),
            Value::Int(7.into())
        );
        assert_eq!(
            require(execute_export(
                &program,
                "truthy",
                vec![Value::Nil],
                ExecutionOptions::default(),
            )),
            Value::Int(1.into())
        );
        let big =
            require("9223372036854775808".parse::<BigInt>().map_err(|_| {
                Diagnostic::simple("TEST_BIGINT", "test integer could not be parsed")
            }));
        assert_eq!(
            require(execute_export(
                &program,
                "big-add",
                vec![Value::Int(big)],
                ExecutionOptions::default(),
            )),
            Value::Int(require("9223372036854775809".parse::<BigInt>().map_err(
                |_| { Diagnostic::simple("TEST_BIGINT", "test integer could not be parsed") }
            )))
        );
    }

    #[test]
    fn returns_stable_runtime_diagnostics() {
        let program = require(load_program_source(CORE));
        let divide = require_error(execute_export(
            &program,
            "divide-by-zero",
            vec![Value::Int(1.into())],
            ExecutionOptions::default(),
        ));
        assert_eq!(divide.code, "RUNTIME_DIVIDE_BY_ZERO");
        assert_eq!(
            divide.message.as_ref(),
            "quotient denominator cannot be zero"
        );

        let arity = require_error(execute_export(
            &program,
            "factorial",
            vec![],
            ExecutionOptions::default(),
        ));
        assert_eq!(arity.code, "RUNTIME_ARITY");
        assert_eq!(
            arity.details.as_ref(),
            &json!({ "name": "function", "minimum": 1, "maximum": 1, "actual": 0 })
        );

        let fuel = require_error(execute_export(
            &program,
            "forever",
            vec![],
            ExecutionOptions {
                fuel: 20,
                maximum_depth: 1000,
                reference_libraries: true,
            },
        ));
        assert_eq!(fuel.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn normalizes_schema_defaults_and_orders_issues() {
        let program = require(load_program_source(SCHEMA));
        let accepted = Value::Map(BTreeMap::from([(
            MapKey::String("id".to_owned()),
            Value::String("a".to_owned()),
        )]));
        assert_eq!(
            require(execute_export(
                &program,
                "check",
                vec![accepted],
                ExecutionOptions::default(),
            )),
            Value::Ok(Box::new(Value::Map(BTreeMap::from([
                (MapKey::String("enabled".to_owned()), Value::Bool(true)),
                (
                    MapKey::String("id".to_owned()),
                    Value::String("a".to_owned())
                ),
            ]))))
        );
    }

    #[test]
    fn executes_text_v1_and_requires_an_explicit_backend() {
        let program = require(load_program_source(LIBRARY));
        assert_eq!(
            require(execute_export(
                &program,
                "inspect",
                vec![Value::String("AI语言".to_owned())],
                ExecutionOptions::default(),
            )),
            Value::List(vec![
                Value::Int(4.into()),
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(true),
                Value::String("机器语言".to_owned()),
            ])
        );
        let unavailable = require_error(execute_export(
            &program,
            "measure",
            vec![Value::String("value".to_owned())],
            ExecutionOptions {
                reference_libraries: false,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(unavailable.code, "RUNTIME_LIBRARY_UNAVAILABLE");
    }

    #[test]
    fn text_v2_matches_compiled_execution_and_obeys_fuel() {
        let program = require(load_program_source(
            r#"(program
                (name text-v2-runtime)
                (version 4)
                (libraries (text 2))
                (signature run (fn (string) string))
                (def run (fn (value)
                  (text/join (text/split (text/trim value) ",") "·")))
                (export run))"#,
        ));
        let input = vec![Value::String("  AI,语言,🦀  ".to_owned())];
        let interpreted = require(execute_export(
            &program,
            "run",
            input.clone(),
            ExecutionOptions::default(),
        ));
        let artifact = require(compile_bytecode(&program));
        let compiled = require(execute_compiled_export(
            &artifact,
            "run",
            input,
            ExecutionOptions::default(),
        ));
        assert_eq!(interpreted, Value::String("AI·语言·🦀".to_owned()));
        assert_eq!(compiled, interpreted);

        let exhausted = require_error(execute_export(
            &program,
            "run",
            vec![Value::String("x,".repeat(10_000))],
            ExecutionOptions {
                fuel: 10,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn math_v1_matches_compiled_execution_and_charges_magnitude_fuel() {
        let program = require(load_program_source(
            r#"(program
                (name math-v1-runtime)
                (version 4)
                (libraries (math 1))
                (signature summarize (fn (integer integer integer integer) map))
                (def summarize
                  (fn (value minimum maximum other)
                    (map "abs" (math/abs value)
                         "sign" (math/sign value)
                         "minimum" (math/min value other)
                         "maximum" (math/max value other)
                         "clamped" (math/clamp value minimum maximum)
                         "gcd" (math/gcd value other))))
                (signature common (fn (integer integer) integer))
                (def common (fn (left right) (math/gcd left right)))
                (export summarize common))"#,
        ));
        let input = vec![
            Value::Int((-42).into()),
            Value::Int((-10).into()),
            Value::Int(10.into()),
            Value::Int(30.into()),
        ];
        let interpreted = require(execute_export(
            &program,
            "summarize",
            input.clone(),
            ExecutionOptions::default(),
        ));
        let artifact = require(compile_bytecode(&program));
        let compiled = require(execute_compiled_export(
            &artifact,
            "summarize",
            input,
            ExecutionOptions::default(),
        ));
        assert_eq!(compiled, interpreted);
        assert_eq!(
            interpreted,
            Value::Map(BTreeMap::from([
                (MapKey::String("abs".to_owned()), Value::Int(42.into())),
                (
                    MapKey::String("clamped".to_owned()),
                    Value::Int((-10).into()),
                ),
                (MapKey::String("gcd".to_owned()), Value::Int(6.into())),
                (MapKey::String("maximum".to_owned()), Value::Int(30.into()),),
                (
                    MapKey::String("minimum".to_owned()),
                    Value::Int((-42).into()),
                ),
                (MapKey::String("sign".to_owned()), Value::Int((-1).into())),
            ]))
        );

        let large = (BigInt::from(1_u8) << 4095_usize) - BigInt::from(1_u8);
        let exhausted = require_error(execute_export(
            &program,
            "common",
            vec![Value::Int(large.clone()), Value::Int(large)],
            ExecutionOptions {
                fuel: 10,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn digest_v1_matches_compiled_execution_and_charges_utf8_byte_fuel() {
        let program = require(load_program_source(
            r#"(program
                (name digest-v1-runtime)
                (version 4)
                (libraries (digest 1))
                (signature hashes (fn (string) map))
                (def hashes
                  (fn (value)
                    (map "sha256" (digest/sha256-text value)
                         "sha512" (digest/sha512-text value))))
                (export hashes))"#,
        ));
        let input = vec![Value::String("abc".to_owned())];
        let interpreted = require(execute_export(
            &program,
            "hashes",
            input.clone(),
            ExecutionOptions::default(),
        ));
        let artifact = require(compile_bytecode(&program));
        let compiled = require(execute_compiled_export(
            &artifact,
            "hashes",
            input,
            ExecutionOptions::default(),
        ));
        assert_eq!(compiled, interpreted);
        assert_eq!(
            interpreted,
            Value::Map(BTreeMap::from([
                (
                    MapKey::String("sha256".to_owned()),
                    Value::String(
                        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                            .to_owned(),
                    ),
                ),
                (
                    MapKey::String("sha512".to_owned()),
                    Value::String(
                        concat!(
                            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
                            "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
                        )
                        .to_owned(),
                    ),
                ),
            ]))
        );

        let exhausted = require_error(execute_export(
            &program,
            "hashes",
            vec![Value::String("x".repeat(10_000))],
            ExecutionOptions {
                fuel: 10,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn json_v1_matches_compiled_execution_and_charges_bounded_work() {
        let program = require(load_program_source(
            r#"(program
                (name json-v1-runtime)
                (version 4)
                (libraries (json 1))
                (signature decode (fn (string) (result any any)))
                (def decode (fn (source) (json/parse source)))
                (signature encode (fn (map) (result any any)))
                (def encode (fn (value) (json/stringify-canonical value)))
                (export decode encode))"#,
        ));
        let source = r#"{"z":2,"a":[true,null]}"#;
        let input = vec![Value::String(source.to_owned())];
        let interpreted = require(execute_export(
            &program,
            "decode",
            input.clone(),
            ExecutionOptions::default(),
        ));
        let artifact = require(compile_bytecode(&program));
        let compiled = require(execute_compiled_export(
            &artifact,
            "decode",
            input,
            ExecutionOptions::default(),
        ));
        assert_eq!(compiled, interpreted);
        assert_eq!(
            interpreted,
            Value::Ok(Box::new(Value::Map(BTreeMap::from([
                (
                    MapKey::String("a".to_owned()),
                    Value::List(vec![Value::Bool(true), Value::Nil]),
                ),
                (MapKey::String("z".to_owned()), Value::Int(2.into())),
            ]))))
        );

        let value = Value::Map(BTreeMap::from([
            (MapKey::String("z".to_owned()), Value::Int(2.into())),
            (
                MapKey::String("a".to_owned()),
                Value::List(vec![Value::Bool(true), Value::Nil]),
            ),
        ]));
        let interpreted_encoding = require(execute_export(
            &program,
            "encode",
            vec![value.clone()],
            ExecutionOptions::default(),
        ));
        let compiled_encoding = require(execute_compiled_export(
            &artifact,
            "encode",
            vec![value],
            ExecutionOptions::default(),
        ));
        assert_eq!(compiled_encoding, interpreted_encoding);
        assert_eq!(
            interpreted_encoding,
            Value::Ok(Box::new(Value::String(
                r#"{"a":[true,null],"z":2}"#.to_owned()
            )))
        );

        let exhausted = require_error(execute_export(
            &program,
            "decode",
            vec![Value::String(" ".repeat(10_000))],
            ExecutionOptions {
                fuel: 10,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn decimal_v1_matches_compiled_execution_and_charges_scale_work() {
        let program = require(load_program_source(
            r#"(program
                (name decimal-v1-runtime)
                (version 4)
                (libraries (decimal 1))
                (signature parse (fn (string integer) (result any any)))
                (def parse (fn (source scale) (decimal/parse-scaled source scale)))
                (signature round (fn (integer integer integer string) (result any any)))
                (def round (fn (value from to mode)
                  (decimal/rescale value from to mode)))
                (export parse round))"#,
        ));
        let interpreted = require(execute_export(
            &program,
            "parse",
            vec![Value::String("12.340".to_owned()), Value::Int(2.into())],
            ExecutionOptions::default(),
        ));
        let artifact = require(compile_bytecode(&program));
        let compiled = require(execute_compiled_export(
            &artifact,
            "parse",
            vec![Value::String("12.340".to_owned()), Value::Int(2.into())],
            ExecutionOptions::default(),
        ));
        assert_eq!(compiled, interpreted);
        assert_eq!(interpreted, Value::Ok(Box::new(Value::Int(1_234.into()))));

        let rounded = require(execute_compiled_export(
            &artifact,
            "round",
            vec![
                Value::Int(125.into()),
                Value::Int(2.into()),
                Value::Int(1.into()),
                Value::String("half-even".to_owned()),
            ],
            ExecutionOptions::default(),
        ));
        assert_eq!(rounded, Value::Ok(Box::new(Value::Int(12.into()))));

        let exhausted = require_error(execute_export(
            &program,
            "round",
            vec![
                Value::Int(1.into()),
                Value::Int(0.into()),
                Value::Int(1_024.into()),
                Value::String("exact".to_owned()),
            ],
            ExecutionOptions {
                fuel: 10,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    struct CountingTextBackend {
        calls: Arc<AtomicUsize>,
    }

    impl LibraryBackend for CountingTextBackend {
        fn descriptor(&self) -> BackendDescriptor {
            BackendDescriptor {
                provider: "test-rust".to_owned(),
                library: "text".to_owned(),
                version: 1,
                operations: vec![
                    "contains?".to_owned(),
                    "ends-with?".to_owned(),
                    "length".to_owned(),
                    "replace".to_owned(),
                    "starts-with?".to_owned(),
                ],
            }
        }

        fn invoke(
            &mut self,
            operation: &str,
            _arguments: &[LibraryValue],
        ) -> YanshuResult<LibraryValue> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if operation == "length" {
                Ok(LibraryValue::Int(99.into()))
            } else {
                Err(Diagnostic::simple(
                    "TEST_UNEXPECTED_OPERATION",
                    "test backend received an unexpected operation",
                ))
            }
        }
    }

    #[test]
    fn supports_explicit_rust_backends_and_charges_fuel_before_invocation() {
        let program = require(load_program_source(LIBRARY));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = LibraryRegistry::default();
        require(registry.register(Box::new(CountingTextBackend {
            calls: Arc::clone(&calls),
        })));
        assert_eq!(
            require(execute_export_with_libraries(
                &program,
                "measure",
                vec![Value::String("value".to_owned())],
                ExecutionOptions::default(),
                &mut registry,
            )),
            Value::Int(99.into())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let exhausted = require_error(execute_export_with_libraries(
            &program,
            "measure",
            vec![Value::String("x".repeat(10_000))],
            ExecutionOptions {
                fuel: 50,
                ..ExecutionOptions::default()
            },
            &mut registry,
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn evaluates_v2_short_circuit_forms_and_number_conversion() {
        let program = require(load_program_source(
            r#"(program
                (name business-rules)
                (version 2)
                (def and-short (fn () (and #f (quotient 1 0))))
                (def or-short (fn () (or 9 (quotient 1 0))))
                (def choose (fn (amount)
                  (cond
                    ((< amount 0) "invalid")
                    ((>= amount 1000) "review")
                    (else (string-append "auto:" (number->string amount))))))
                (export and-short or-short choose))"#,
        ));
        assert_eq!(
            require(execute_export(
                &program,
                "and-short",
                vec![],
                ExecutionOptions::default(),
            )),
            Value::Bool(false)
        );
        assert_eq!(
            require(execute_export(
                &program,
                "or-short",
                vec![],
                ExecutionOptions::default(),
            )),
            Value::Int(9.into())
        );
        assert_eq!(
            require(execute_export(
                &program,
                "choose",
                vec![Value::Int(42.into())],
                ExecutionOptions::default(),
            )),
            Value::String("auto:42".to_owned())
        );
    }

    #[test]
    fn constructs_and_matches_v3_user_data() {
        let program = require(load_program_source(
            r#"(program
                (name decisions)
                (version 3)
                (data decision (approved id) (rejected reason))
                (def approve (fn (id) (approved id)))
                (def describe (fn (decision)
                  (match decision
                    ((approved id) (string-append "approved:" id))
                    ((rejected reason) (string-append "rejected:" reason))
                    (_ "unknown"))))
                (def run (fn (id) (describe (approve id))))
                (export run))"#,
        ));
        assert_eq!(
            require(execute_export(
                &program,
                "run",
                vec![Value::String("expense-42".to_owned())],
                ExecutionOptions::default(),
            )),
            Value::String("approved:expense-42".to_owned())
        );
    }

    #[test]
    fn refuses_to_execute_an_unlinked_module() {
        let program = require(load_program_source(
            "(program (name app) (version 3) (imports helper) (def run #t) (export run))",
        ));
        let diagnostic = require_error(execute_export(
            &program,
            "run",
            vec![],
            ExecutionOptions::default(),
        ));
        assert_eq!(diagnostic.code, "RUNTIME_UNLINKED_IMPORTS");
    }

    #[test]
    fn enforces_v4_static_types_before_execution() {
        let valid = require(load_program_source(
            r#"(program
                (name typed-runtime)
                (version 4)
                (signature double (fn (integer) integer))
                (def double (fn (value) (+ value value)))
                (export double))"#,
        ));
        assert_eq!(
            require(execute_export(
                &valid,
                "double",
                vec![Value::Int(21.into())],
                ExecutionOptions::default(),
            )),
            Value::Int(42.into())
        );
        let diagnostic = require_error(execute_export(
            &valid,
            "double",
            vec![Value::String("21".to_owned())],
            ExecutionOptions::default(),
        ));
        assert_eq!(diagnostic.code, "TYPE_INPUT_MISMATCH");

        let invalid = require(load_program_source(
            r#"(program
                (name invalid-runtime)
                (version 4)
                (signature run (fn (integer) integer))
                (def run (fn (value) (string-append "value:" value)))
                (export run))"#,
        ));
        let diagnostic = require_error(execute_export(
            &invalid,
            "run",
            vec![Value::Int(1.into())],
            ExecutionOptions::default(),
        ));
        assert_eq!(diagnostic.code, "TYPE_MISMATCH");

        let dynamic_output = require(load_program_source(
            r#"(program
                (name dynamic-output)
                (version 4)
                (signature run (fn (map) integer))
                (def run (fn (value) (get value "result")))
                (export run))"#,
        ));
        let diagnostic = require_error(execute_export(
            &dynamic_output,
            "run",
            vec![Value::Map(BTreeMap::from([(
                MapKey::String("result".to_owned()),
                Value::String("not-an-integer".to_owned()),
            )]))],
            ExecutionOptions::default(),
        ));
        assert_eq!(diagnostic.code, "TYPE_OUTPUT_MISMATCH");
    }

    #[test]
    fn validates_enum_and_union_with_machine_readable_cost() {
        let program = require(load_program_source(
            r#"(program
                (name typed-decisions)
                (version 2)
                (schema action (enum "approve" "reject"))
                (schema identity (union integer string))
                (def check-action (fn (value) (validate-report action value)))
                (def check-identity (fn (value) (validate-report identity value)))
                (export check-action check-identity))"#,
        ));

        let rejected = require(execute_export(
            &program,
            "check-action",
            vec![Value::String("hold".to_owned())],
            ExecutionOptions::default(),
        ));
        let Value::Map(report) = rejected else {
            panic!("expected validation report");
        };
        assert_eq!(
            report.get(&MapKey::String("valid".to_owned())),
            Some(&Value::Bool(false))
        );
        assert_eq!(
            report
                .get(&MapKey::String("cost".to_owned()))
                .and_then(|value| match value {
                    Value::Map(cost) => cost.get(&MapKey::String("fuel".to_owned())),
                    _ => None,
                }),
            Some(&Value::Int(5.into()))
        );
        let issue_code = report
            .get(&MapKey::String("issues".to_owned()))
            .and_then(|value| match value {
                Value::List(issues) => issues.first(),
                _ => None,
            })
            .and_then(|value| match value {
                Value::Map(issue) => issue.get(&MapKey::String("code".to_owned())),
                _ => None,
            });
        assert_eq!(issue_code, Some(&Value::String("SCHEMA_ENUM".to_owned())));

        let accepted = require(execute_export(
            &program,
            "check-identity",
            vec![Value::String("expense-42".to_owned())],
            ExecutionOptions::default(),
        ));
        let Value::Map(report) = accepted else {
            panic!("expected validation report");
        };
        assert_eq!(
            report.get(&MapKey::String("valid".to_owned())),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            report
                .get(&MapKey::String("cost".to_owned()))
                .and_then(|value| match value {
                    Value::Map(cost) => cost.get(&MapKey::String("fuel".to_owned())),
                    _ => None,
                }),
            Some(&Value::Int(7.into()))
        );
    }

    #[test]
    fn transforms_and_aggregates_lists_with_fuel_charged_per_item() {
        let program = require(load_program_source(
            r#"(program
                (name expense-collections)
                (version 2)
                (def total (fn (items) (sum items)))
                (def boosted (fn (items)
                  (list-map (fn (amount) (+ amount 10)) items)))
                (def positive (fn (items)
                  (list-filter (fn (amount) (> amount 0)) items)))
                (def folded (fn (items)
                  (list-fold (fn (total amount) (+ total amount)) 0 items)))
                (export total boosted positive folded))"#,
        ));
        let items = Value::List(vec![
            Value::Int(1.into()),
            Value::Int(2.into()),
            Value::Int(3.into()),
        ]);
        assert_eq!(
            require(execute_export(
                &program,
                "total",
                vec![items.clone()],
                ExecutionOptions::default(),
            )),
            Value::Int(6.into())
        );
        assert_eq!(
            require(execute_export(
                &program,
                "boosted",
                vec![items.clone()],
                ExecutionOptions::default(),
            )),
            Value::List(vec![
                Value::Int(11.into()),
                Value::Int(12.into()),
                Value::Int(13.into()),
            ])
        );
        assert_eq!(
            require(execute_export(
                &program,
                "positive",
                vec![Value::List(vec![
                    Value::Int((-1).into()),
                    Value::Int(0.into()),
                    Value::Int(2.into()),
                ])],
                ExecutionOptions::default(),
            )),
            Value::List(vec![Value::Int(2.into())])
        );
        assert_eq!(
            require(execute_export(
                &program,
                "folded",
                vec![items],
                ExecutionOptions::default(),
            )),
            Value::Int(6.into())
        );

        let exhausted = require_error(execute_export(
            &program,
            "total",
            vec![Value::List(
                (0..200).map(|value| Value::Int(value.into())).collect(),
            )],
            ExecutionOptions {
                fuel: 100,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(exhausted.code, "RUNTIME_FUEL_EXHAUSTED");
    }

    #[test]
    fn degrades_only_explicit_business_results() {
        let program = require(load_program_source(
            r#"(program
                (name recoverable-arithmetic)
                (version 2)
                (def ratio (fn (amount count)
                  (let ((attempt (checked-quotient amount count)))
                    (if (ok? attempt) (result-value attempt) -1))))
                (def checked (fn (amount count)
                  (checked-remainder amount count)))
                (def raw (fn (amount count) (quotient amount count)))
                (export ratio checked raw))"#,
        ));
        assert_eq!(
            require(execute_export(
                &program,
                "ratio",
                vec![Value::Int(100.into()), Value::Int(0.into())],
                ExecutionOptions::default(),
            )),
            Value::Int((-1).into())
        );
        assert_eq!(
            require(execute_export(
                &program,
                "checked",
                vec![Value::Int(10.into()), Value::Int(3.into())],
                ExecutionOptions::default(),
            )),
            Value::Ok(Box::new(Value::Int(1.into())))
        );
        let raw = require_error(execute_export(
            &program,
            "raw",
            vec![Value::Int(1.into()), Value::Int(0.into())],
            ExecutionOptions::default(),
        ));
        assert_eq!(raw.code, "RUNTIME_DIVIDE_BY_ZERO");

        let fuel = require_error(execute_export(
            &program,
            "ratio",
            vec![Value::Int(100.into()), Value::Int(2.into())],
            ExecutionOptions {
                fuel: 2,
                ..ExecutionOptions::default()
            },
        ));
        assert_eq!(fuel.code, "RUNTIME_FUEL_EXHAUSTED");
    }
}
