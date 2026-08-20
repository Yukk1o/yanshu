#![forbid(unsafe_code)]

mod ast;
mod parser;
mod reader;

pub use ast::{
    Binding, CondClause, DataField, DataTypeDefinition, Datum, DatumKind, Definition, Expression,
    ExpressionKind, FunctionSignature, LibraryRequirement, MatchArm, Pattern, PatternKind, Program,
    Route, Schema, SchemaField, SchemaKind, TypeExpression, VariantDefinition,
};
pub use parser::parse_program;
pub use reader::{ReaderLimits, read_source};

use yanshu_diagnostic::YanshuResult;

pub fn load_program_source(source: &str) -> YanshuResult<Program> {
    let datum = read_source(source, ReaderLimits::default())?;
    parse_program(&datum, source)
}
