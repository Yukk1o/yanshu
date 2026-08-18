#![forbid(unsafe_code)]

mod ast;
mod parser;
mod reader;

pub use ast::{
    Binding, CondClause, DataTypeDefinition, Datum, DatumKind, Definition, Expression,
    ExpressionKind, LibraryRequirement, MatchArm, Pattern, PatternKind, Program, Route, Schema,
    SchemaField, SchemaKind, VariantDefinition,
};
pub use parser::parse_program;
pub use reader::{ReaderLimits, read_source};

use ail_diagnostic::AilResult;

pub fn load_program_source(source: &str) -> AilResult<Program> {
    let datum = read_source(source, ReaderLimits::default())?;
    parse_program(&datum, source)
}
