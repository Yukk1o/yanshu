#![forbid(unsafe_code)]

mod cst;
mod render;

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};
use yanshu_syntax::{Program, ReaderLimits, parse_program, read_source};

use crate::{cst::ConcreteParser, render::render_document};

pub const FORMATTER_VERSION: u32 = 1;
const MINIMUM_LINE_WIDTH: usize = 40;
const MAXIMUM_LINE_WIDTH: usize = 240;
const MINIMUM_INDENT_WIDTH: usize = 1;
const MAXIMUM_INDENT_WIDTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    pub line_width: usize,
    pub indent_width: usize,
    pub max_output_bytes: usize,
    pub reader_limits: ReaderLimits,
}

impl Default for FormatOptions {
    fn default() -> Self {
        let reader_limits = ReaderLimits::default();
        Self {
            line_width: 100,
            indent_width: 2,
            max_output_bytes: reader_limits.max_source_bytes,
            reader_limits,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedSource {
    pub source: String,
    pub changed: bool,
}

/// Formats one complete `.yan` program without changing its parsed meaning.
///
/// The existing language parser validates both documents. A formatter bug is
/// therefore fail-closed rather than allowed to rewrite canonical source.
pub fn format_source(source: &str, options: FormatOptions) -> YanshuResult<FormattedSource> {
    validate_options(options)?;
    if source.len() > options.reader_limits.max_source_bytes {
        return Err(Diagnostic::new(
            "FORMAT_SOURCE_LIMIT",
            "source exceeds the formatter byte limit",
            json!({
                "actual": source.len(),
                "maximum": options.reader_limits.max_source_bytes,
            }),
        ));
    }

    let original = parse_with_limits(source, options.reader_limits)?;
    let document = ConcreteParser::new(source).parse_document()?;
    let original_comments = document.comments();
    let formatted = render_document(&document, options)?;
    let reparsed = parse_with_limits(&formatted, options.reader_limits).map_err(|diagnostic| {
        Diagnostic::new(
            "FORMAT_INTERNAL_INVALID_OUTPUT",
            "formatter output did not parse as the original language version",
            json!({ "parserCode": diagnostic.code }),
        )
    })?;
    if original.inspect_json() != reparsed.inspect_json() {
        return Err(Diagnostic::simple(
            "FORMAT_SEMANTIC_MISMATCH",
            "formatter output changed the parsed program",
        ));
    }

    let formatted_document = ConcreteParser::new(&formatted).parse_document()?;
    if original_comments != formatted_document.comments() {
        return Err(Diagnostic::simple(
            "FORMAT_COMMENT_MISMATCH",
            "formatter output changed or removed source comments",
        ));
    }
    let second_pass = render_document(&formatted_document, options)?;
    if second_pass != formatted {
        return Err(Diagnostic::simple(
            "FORMAT_NOT_IDEMPOTENT",
            "formatter output was not stable on a second pass",
        ));
    }

    Ok(FormattedSource {
        changed: source != formatted,
        source: formatted,
    })
}

fn validate_options(options: FormatOptions) -> YanshuResult<()> {
    if !(MINIMUM_LINE_WIDTH..=MAXIMUM_LINE_WIDTH).contains(&options.line_width) {
        return Err(Diagnostic::new(
            "FORMAT_LINE_WIDTH",
            "formatter line width is outside the supported range",
            json!({
                "actual": options.line_width,
                "minimum": MINIMUM_LINE_WIDTH,
                "maximum": MAXIMUM_LINE_WIDTH,
            }),
        ));
    }
    if !(MINIMUM_INDENT_WIDTH..=MAXIMUM_INDENT_WIDTH).contains(&options.indent_width) {
        return Err(Diagnostic::new(
            "FORMAT_INDENT_WIDTH",
            "formatter indent width is outside the supported range",
            json!({
                "actual": options.indent_width,
                "minimum": MINIMUM_INDENT_WIDTH,
                "maximum": MAXIMUM_INDENT_WIDTH,
            }),
        ));
    }
    if options.max_output_bytes == 0
        || options.max_output_bytes > options.reader_limits.max_source_bytes
    {
        return Err(Diagnostic::new(
            "FORMAT_OUTPUT_LIMIT",
            "formatter output byte limit must be positive and within the Reader source limit",
            json!({
                "actual": options.max_output_bytes,
                "maximum": options.reader_limits.max_source_bytes,
            }),
        ));
    }
    Ok(())
}

fn parse_with_limits(source: &str, limits: ReaderLimits) -> YanshuResult<Program> {
    let datum = read_source(source, limits)?;
    parse_program(&datum, source)
}

#[cfg(test)]
mod tests;
