#![forbid(unsafe_code)]

use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    LibraryKey, LibraryValue, MAXIMUM_LIBRARY_INTEGER_BITS, MAXIMUM_LIBRARY_VALUE_BYTES,
    MAXIMUM_LIBRARY_VALUE_DEPTH, MAXIMUM_LIBRARY_VALUE_NODES,
};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Metrics {
    nodes: usize,
    scalar_bytes: usize,
    integer_bits: u64,
}

impl Metrics {
    pub(crate) fn single_node() -> Self {
        Self {
            nodes: 1,
            ..Self::default()
        }
    }

    pub(crate) fn add(&mut self, other: Self, context: LimitContext) -> YanshuResult<()> {
        self.nodes = self.nodes.saturating_add(other.nodes);
        self.scalar_bytes = self.scalar_bytes.saturating_add(other.scalar_bytes);
        self.integer_bits = self.integer_bits.saturating_add(other.integer_bits);
        self.check(context)
    }

    fn add_scalar_bytes(&mut self, bytes: usize, context: LimitContext) -> YanshuResult<()> {
        self.scalar_bytes = self.scalar_bytes.saturating_add(bytes);
        self.check(context)
    }

    fn check(self, context: LimitContext) -> YanshuResult<()> {
        if self.nodes > MAXIMUM_LIBRARY_VALUE_NODES {
            return Err(limit_error(
                context,
                "nodes",
                MAXIMUM_LIBRARY_VALUE_NODES,
                self.nodes,
            ));
        }
        if self.scalar_bytes > MAXIMUM_LIBRARY_VALUE_BYTES {
            return Err(limit_error(
                context,
                "scalarBytes",
                MAXIMUM_LIBRARY_VALUE_BYTES,
                self.scalar_bytes,
            ));
        }
        Ok(())
    }

    pub(crate) fn work(self) -> u64 {
        u64::try_from(self.nodes)
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(self.scalar_bytes).unwrap_or(u64::MAX))
            .saturating_add(self.integer_bits)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LimitContext {
    Argument,
    Result,
}

pub(crate) fn measure_arguments(
    arguments: &[LibraryValue],
    context: LimitContext,
) -> YanshuResult<Metrics> {
    let mut total = Metrics::default();
    for argument in arguments {
        let metrics = measure_value(argument, 0, context)?;
        total.nodes = total.nodes.saturating_add(metrics.nodes);
        total.scalar_bytes = total.scalar_bytes.saturating_add(metrics.scalar_bytes);
        total.integer_bits = total.integer_bits.saturating_add(metrics.integer_bits);
    }
    Ok(total)
}

pub(crate) fn measure_ok_value(
    value: &LibraryValue,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    let mut metrics = Metrics::single_node();
    metrics.add(measure_value(value, 1, context)?, context)?;
    Ok(metrics)
}

pub(crate) fn measure_ok_list(
    values: &[LibraryValue],
    context: LimitContext,
) -> YanshuResult<Metrics> {
    let mut metrics = Metrics::single_node();
    metrics.add(measure_list(values, 1, context)?, context)?;
    Ok(metrics)
}

pub(crate) fn measure_list(
    values: &[LibraryValue],
    depth: usize,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    measure_list_iter(values.iter(), values.len(), depth, context)
}

pub(crate) fn measure_list_iter<'a>(
    values: impl Iterator<Item = &'a LibraryValue>,
    length: usize,
    depth: usize,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    check_collection(depth, length, context)?;
    let mut metrics = Metrics::single_node();
    for value in values {
        metrics.add(measure_value(value, depth + 1, context)?, context)?;
    }
    Ok(metrics)
}

pub(crate) fn measure_map_iter<'a>(
    values: impl Iterator<Item = (&'a LibraryKey, &'a LibraryValue)>,
    length: usize,
    depth: usize,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    check_collection(depth, length, context)?;
    let mut metrics = Metrics::single_node();
    for (key, value) in values {
        metrics.add_scalar_bytes(key_text(key).len(), context)?;
        metrics.add(measure_value(value, depth + 1, context)?, context)?;
    }
    Ok(metrics)
}

pub(crate) fn measure_key_value(
    key: &LibraryKey,
    depth: usize,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    check_depth(depth, context)?;
    let mut metrics = Metrics::single_node();
    metrics.add_scalar_bytes(key_text(key).len(), context)?;
    Ok(metrics)
}

pub(crate) fn measure_value(
    value: &LibraryValue,
    depth: usize,
    context: LimitContext,
) -> YanshuResult<Metrics> {
    check_depth(depth, context)?;
    let mut metrics = Metrics::single_node();
    match value {
        LibraryValue::Nil | LibraryValue::Bool(_) => {}
        LibraryValue::Int(value) => {
            let bits = value.bits();
            if bits > MAXIMUM_LIBRARY_INTEGER_BITS {
                return Err(limit_error_u64(
                    context,
                    "integerBits",
                    MAXIMUM_LIBRARY_INTEGER_BITS,
                    bits,
                ));
            }
            metrics.integer_bits = bits;
            metrics.scalar_bytes = usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX);
        }
        LibraryValue::String(value) | LibraryValue::Symbol(value) => {
            metrics.scalar_bytes = value.len();
        }
        LibraryValue::List(values) => return measure_list(values, depth, context),
        LibraryValue::Map(values) => {
            return measure_map_iter(values.iter(), values.len(), depth, context);
        }
        LibraryValue::Ok(value) | LibraryValue::Err(value) => {
            metrics.add(measure_value(value, depth + 1, context)?, context)?;
        }
        LibraryValue::Variant {
            type_name,
            variant,
            fields,
        } => {
            check_collection(depth, fields.len(), context)?;
            metrics.add_scalar_bytes(type_name.len().saturating_add(variant.len()), context)?;
            for field in fields {
                metrics.add(measure_value(field, depth + 1, context)?, context)?;
            }
        }
    }
    metrics.check(context)?;
    Ok(metrics)
}

fn check_collection(depth: usize, length: usize, context: LimitContext) -> YanshuResult<()> {
    check_depth(depth, context)?;
    if length > MAXIMUM_LIBRARY_VALUE_NODES {
        Err(limit_error(
            context,
            "collectionItems",
            MAXIMUM_LIBRARY_VALUE_NODES,
            length,
        ))
    } else {
        Ok(())
    }
}

fn check_depth(depth: usize, context: LimitContext) -> YanshuResult<()> {
    if depth > MAXIMUM_LIBRARY_VALUE_DEPTH {
        Err(limit_error(
            context,
            "depth",
            MAXIMUM_LIBRARY_VALUE_DEPTH,
            depth,
        ))
    } else {
        Ok(())
    }
}

fn key_text(key: &LibraryKey) -> &str {
    match key {
        LibraryKey::String(value) | LibraryKey::Symbol(value) => value,
    }
}

fn limit_error(
    context: LimitContext,
    kind: &'static str,
    maximum: usize,
    actual: usize,
) -> Diagnostic {
    let (code, message) = limit_identity(context);
    Diagnostic::new(
        code,
        message,
        json!({ "kind": kind, "maximum": maximum, "actual": actual }),
    )
}

fn limit_error_u64(
    context: LimitContext,
    kind: &'static str,
    maximum: u64,
    actual: u64,
) -> Diagnostic {
    let (code, message) = limit_identity(context);
    Diagnostic::new(
        code,
        message,
        json!({ "kind": kind, "maximum": maximum, "actual": actual }),
    )
}

fn limit_identity(context: LimitContext) -> (&'static str, &'static str) {
    match context {
        LimitContext::Argument => (
            "RUNTIME_LIBRARY_ARGUMENT",
            "library operation argument exceeds the portable value envelope",
        ),
        LimitContext::Result => (
            "RUNTIME_LIBRARY_RESULT_LIMIT",
            "library operation result exceeds the portable value envelope",
        ),
    }
}
