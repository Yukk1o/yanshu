#![forbid(unsafe_code)]

mod ast;
mod node_id;
mod parser;
mod reader;
mod symbol;

pub use ast::{
    Binding, CondClause, DataField, DataTypeDefinition, Datum, DatumKind, Definition, Expression,
    ExpressionKind, FunctionSignature, LibraryRequirement, MatchArm, Pattern, PatternKind, Program,
    Route, Schema, SchemaField, SchemaKind, TypeExpression, VariantDefinition,
};
pub use node_id::{ExpressionNode, expression_nodes};
pub use parser::parse_program;
pub use reader::{ReaderLimits, read_source};
pub use symbol::{LocalBinding, LocalBindingKind, LocalSymbolIndex, local_symbol_index};

use yanshu_diagnostic::YanshuResult;

pub fn load_program_source(source: &str) -> YanshuResult<Program> {
    let datum = read_source(source, ReaderLimits::default())?;
    parse_program(&datum, source)
}
