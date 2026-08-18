#![forbid(unsafe_code)]

mod budget;
mod schema;
mod value;

use std::collections::BTreeMap;

use ail_diagnostic::{AilResult, Diagnostic};
use ail_syntax::{Expression, ExpressionKind, Program};
use num_bigint::BigInt;
use num_traits::{One, Zero};
use serde_json::{Value as JsonValue, json};

pub use budget::Budget;
pub use value::{MapKey, Primitive, PrimitiveOperation, Value, bigint_json};

use schema::validate_schema;

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

pub fn execute_export(
    program: &Program,
    export_name: &str,
    arguments: Vec<Value>,
    options: ExecutionOptions,
) -> AilResult<Value> {
    if !program.exports.iter().any(|name| name == export_name) {
        return Err(Diagnostic::new(
            "RUNTIME_NOT_EXPORTED",
            "requested entry point is not exported",
            json!({ "name": export_name }),
        ));
    }
    let mut runtime = Runtime::new(options);
    let base_environment = runtime.new_environment(None);
    runtime.install_base_environment(base_environment);
    let module_environment = runtime.new_environment(Some(base_environment));
    runtime.install_libraries(program, module_environment)?;
    for schema in &program.schemas {
        runtime.define(
            module_environment,
            schema.name.clone(),
            Value::Schema {
                name: schema.name.clone(),
                specification: schema.kind.clone(),
            },
        );
    }
    for definition in &program.definitions {
        let value = runtime.evaluate(&definition.expression, module_environment, 0)?;
        runtime.define(module_environment, definition.name.clone(), value);
    }
    let callable = runtime.lookup(module_environment, export_name)?;
    runtime.apply(callable, arguments, 0)
}

#[derive(Debug, Clone)]
struct Environment {
    bindings: BTreeMap<String, Value>,
    parent: Option<usize>,
}

#[derive(Debug, Clone)]
struct Closure {
    parameters: Vec<String>,
    body: Expression,
    environment: usize,
}

struct Runtime {
    budget: Budget,
    options: ExecutionOptions,
    environments: Vec<Environment>,
    closures: Vec<Closure>,
}

impl Runtime {
    fn new(options: ExecutionOptions) -> Self {
        Self {
            budget: Budget::new(options.fuel, options.maximum_depth),
            options,
            environments: Vec::new(),
            closures: Vec::new(),
        }
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

    fn lookup(&self, environment: usize, name: &str) -> AilResult<Value> {
        let mut current = Some(environment);
        while let Some(identifier) = current {
            let Some(target) = self.environments.get(identifier) else {
                break;
            };
            if let Some(value) = target.bindings.get(name) {
                return Ok(value.clone());
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
    ) -> AilResult<Value> {
        self.budget.consume(1)?;
        self.budget.check_depth(depth)?;
        match &expression.kind {
            ExpressionKind::Literal(datum) | ExpressionKind::Quote(datum) => Ok(Value::from(datum)),
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
                self.closures.push(Closure {
                    parameters: parameters.clone(),
                    body: body.as_ref().clone(),
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
                    .collect::<AilResult<Vec<_>>>()?;
                self.apply(callable, values, depth + 1)
            }
        }
    }

    fn apply(&mut self, callable: Value, arguments: Vec<Value>, depth: usize) -> AilResult<Value> {
        self.budget.consume(1)?;
        self.budget.check_depth(depth)?;
        match callable {
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
                self.evaluate(&closure.body, call_environment, depth)
            }
            Value::Primitive(primitive) => {
                check_arity(primitive, arguments.len())?;
                self.apply_primitive(primitive, arguments)
            }
            value => Err(Diagnostic::new(
                "RUNTIME_NOT_CALLABLE",
                "attempted to call a non-callable value",
                json!({ "kind": value.kind() }),
            )),
        }
    }

    fn install_base_environment(&mut self, environment: usize) {
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
    }

    fn install_libraries(&mut self, program: &Program, environment: usize) -> AilResult<()> {
        use PrimitiveOperation as Operation;
        for requirement in &program.libraries {
            if !self.options.reference_libraries {
                return Err(Diagnostic::new(
                    "RUNTIME_LIBRARY_UNAVAILABLE",
                    "host did not provide a declared library backend",
                    json!({ "library": requirement.name, "version": requirement.version }),
                ));
            }
            if requirement.name != "text" || requirement.version != 1 {
                return Err(Diagnostic::new(
                    "RUNTIME_LIBRARY_CONTRACT_MISSING",
                    "parsed program refers to an unknown library contract",
                    json!({ "library": requirement.name, "version": requirement.version }),
                ));
            }
            let operations = [
                primitive("text/length", 1, Some(1), Operation::TextLength),
                primitive("text/starts-with?", 2, Some(2), Operation::TextStartsWith),
                primitive("text/ends-with?", 2, Some(2), Operation::TextEndsWith),
                primitive("text/contains?", 2, Some(2), Operation::TextContains),
                primitive("text/replace", 3, Some(3), Operation::TextReplace),
            ];
            for item in operations {
                self.define(environment, item.name.to_owned(), Value::Primitive(item));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn apply_primitive(&mut self, primitive: Primitive, arguments: Vec<Value>) -> AilResult<Value> {
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
                    result *= expect_integer(primitive.name, argument)?;
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
            Operation::Quotient | Operation::Remainder => {
                let numerator = expect_integer(primitive.name, &arguments[0])?;
                let denominator = expect_integer(primitive.name, &arguments[1])?;
                if denominator.is_zero() {
                    return Err(Diagnostic::simple(
                        "RUNTIME_DIVIDE_BY_ZERO",
                        if primitive.operation == Operation::Quotient {
                            "quotient denominator cannot be zero"
                        } else {
                            "remainder denominator cannot be zero"
                        },
                    ));
                }
                Ok(Value::Int(if primitive.operation == Operation::Quotient {
                    numerator / denominator
                } else {
                    numerator % denominator
                }))
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
                let mut result = String::new();
                for argument in &arguments {
                    result.push_str(expect_string(primitive.name, argument)?);
                }
                Ok(Value::String(result))
            }
            Operation::List => Ok(list_value(arguments)),
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
            Operation::TextLength
            | Operation::TextStartsWith
            | Operation::TextEndsWith
            | Operation::TextContains
            | Operation::TextReplace => self.apply_text_primitive(primitive, &arguments),
        }
    }

    fn apply_text_primitive(
        &mut self,
        primitive: Primitive,
        arguments: &[Value],
    ) -> AilResult<Value> {
        let strings = arguments
            .iter()
            .enumerate()
            .map(|(index, value)| match value {
                Value::String(text) => Ok(text.as_str()),
                _ => Err(Diagnostic::new(
                    "RUNTIME_TYPE",
                    "library function received a value of the wrong type",
                    json!({
                        "operation": primitive.name,
                        "index": index,
                        "expected": "String",
                        "actual": value.kind(),
                    }),
                )),
            })
            .collect::<AilResult<Vec<_>>>()?;
        let characters = strings
            .iter()
            .map(|value| value.chars().count())
            .sum::<usize>();
        let blocks = characters.saturating_add(63) / 64;
        self.budget
            .consume(1 + u64::try_from(blocks).unwrap_or(u64::MAX))?;
        let result = match primitive.operation {
            PrimitiveOperation::TextLength => Value::Int(strings[0].chars().count().into()),
            PrimitiveOperation::TextStartsWith => Value::Bool(strings[0].starts_with(strings[1])),
            PrimitiveOperation::TextEndsWith => Value::Bool(strings[0].ends_with(strings[1])),
            PrimitiveOperation::TextContains => Value::Bool(strings[0].contains(strings[1])),
            PrimitiveOperation::TextReplace => {
                Value::String(strings[0].replace(strings[1], strings[2]))
            }
            _ => {
                return Err(Diagnostic::simple(
                    "RUNTIME_LIBRARY_CONTRACT_FAILURE",
                    "library operation is not part of its contract",
                ));
            }
        };
        self.normalize_library_result(&result, 0, &mut 0)
    }

    fn normalize_library_result(
        &mut self,
        value: &Value,
        depth: usize,
        node_count: &mut usize,
    ) -> AilResult<Value> {
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
                    .collect::<AilResult<Vec<_>>>()?,
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

fn check_arity(primitive: Primitive, actual: usize) -> AilResult<()> {
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

fn expect_integer<'value>(operation: &str, value: &'value Value) -> AilResult<&'value BigInt> {
    match value {
        Value::Int(integer) => Ok(integer),
        _ => Err(type_error(operation, "Int", value)),
    }
}

fn expect_string<'value>(operation: &str, value: &'value Value) -> AilResult<&'value str> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(type_error(operation, "String", value)),
    }
}

fn expect_list<'value>(operation: &str, value: &'value Value) -> AilResult<&'value [Value]> {
    match value {
        Value::Nil => Ok(&[]),
        Value::List(values) => Ok(values),
        _ => Err(type_error(operation, "List", value)),
    }
}

fn expect_map<'value>(
    operation: &str,
    value: &'value Value,
) -> AilResult<&'value BTreeMap<MapKey, Value>> {
    match value {
        Value::Map(mapping) => Ok(mapping),
        _ => Err(type_error(operation, "Map", value)),
    }
}

fn expect_map_key(operation: &str, value: &Value) -> AilResult<MapKey> {
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

fn build_map(arguments: Vec<Value>) -> AilResult<Value> {
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

fn expect_status(operation: &str, value: &Value, minimum: i64) -> AilResult<BigInt> {
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

fn build_api_error(arguments: &[Value]) -> AilResult<Value> {
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
    use std::collections::BTreeMap;

    use ail_diagnostic::{AilResult, Diagnostic};
    use ail_syntax::load_program_source;
    use num_bigint::BigInt;
    use serde_json::json;

    use super::{ExecutionOptions, MapKey, Value, execute_export};

    const CORE: &str = include_str!("../../../../conformance/v1/programs/core.ail");
    const SCHEMA: &str = include_str!("../../../../conformance/v1/programs/schema.ail");
    const LIBRARY: &str = include_str!("../../../../conformance/v1/programs/library.ail");

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
}
