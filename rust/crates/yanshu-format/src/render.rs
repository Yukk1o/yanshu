use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

use crate::{
    FormatOptions,
    cst::{Document, Item, Node, first_node},
};

pub(crate) fn render_document(document: &Document, options: FormatOptions) -> YanshuResult<String> {
    let mut writer = Writer::new(options);
    for comment in &document.leading_comments {
        writer.write(comment)?;
        writer.newline(0, false)?;
    }
    render_node(&document.root, 0, &mut writer)?;
    for comment in &document.trailing_comments {
        writer.newline(0, false)?;
        writer.write(comment)?;
    }
    writer.finish()
}

fn render_node(node: &Node, indent: usize, writer: &mut Writer) -> YanshuResult<()> {
    if let Some(length) = flat_length(node)
        && writer.column.saturating_add(length) <= writer.options.line_width
    {
        return render_flat(node, writer);
    }
    match node {
        Node::Atom(value) | Node::String(value) => writer.write(value),
        Node::Quote { comments, value } => {
            writer.write("'")?;
            for comment in comments {
                writer.newline(indent + writer.options.indent_width, false)?;
                writer.write(comment)?;
            }
            writer.newline(indent + writer.options.indent_width, false)?;
            render_node(value, indent + writer.options.indent_width, writer)
        }
        Node::List(items) => render_list(items, indent, writer),
    }
}

fn render_list(items: &[Item], indent: usize, writer: &mut Writer) -> YanshuResult<()> {
    if let Some((bindings, body)) = let_parts(items) {
        return render_let(bindings, body, indent, writer);
    }
    if let Some(pairs) = map_pairs(items) {
        return render_map(pairs, indent, writer);
    }

    writer.write("(")?;
    if items.is_empty() {
        return writer.write(")");
    }

    let head = first_node(items).and_then(|node| match node {
        Node::Atom(value) => Some(value.as_str()),
        Node::String(_) | Node::Quote { .. } | Node::List(_) => None,
    });
    let prefix_nodes = preferred_prefix_nodes(head);
    let is_program = head == Some("program");
    let child_indent = indent + writer.options.indent_width;
    let mut nodes_seen = 0_usize;
    let mut multiline = false;
    let mut last_was_comment = false;
    let mut previous_program_category = None;

    for item in items {
        let can_prefix = match item {
            Item::Node(node) if nodes_seen < prefix_nodes && !multiline => flat_length(node)
                .is_some_and(|length| {
                    let separator = usize::from(nodes_seen > 0);
                    writer
                        .column
                        .saturating_add(separator)
                        .saturating_add(length)
                        <= writer.options.line_width
                }),
            Item::Node(_) | Item::Comment(_) => false,
        };
        if can_prefix {
            if nodes_seen > 0 {
                writer.write(" ")?;
            }
            let Item::Node(node) = item else {
                return Err(Diagnostic::simple(
                    "FORMAT_INTERNAL_LAYOUT",
                    "formatter prefix layout lost a syntax node",
                ));
            };
            render_flat(node, writer)?;
            nodes_seen += 1;
            last_was_comment = false;
            continue;
        }

        if nodes_seen == 0
            && !multiline
            && let Item::Node(node) = item
        {
            render_node(node, indent + 1, writer)?;
            nodes_seen = 1;
            last_was_comment = false;
            continue;
        }

        let blank = if is_program {
            match item {
                Item::Node(node) => {
                    let category = program_category(node);
                    let blank = previous_program_category.is_some_and(|previous| {
                        previous != category || category == ProgramCategory::Definition
                    });
                    previous_program_category = Some(category);
                    blank
                }
                Item::Comment(_) => false,
            }
        } else {
            false
        };
        writer.newline(child_indent, blank)?;
        multiline = true;
        match item {
            Item::Node(node) => {
                render_node(node, child_indent, writer)?;
                nodes_seen += 1;
                last_was_comment = false;
            }
            Item::Comment(comment) => {
                writer.write(comment)?;
                last_was_comment = true;
            }
        }
    }
    if last_was_comment {
        writer.newline(indent, false)?;
    }
    writer.write(")")
}

fn let_parts(items: &[Item]) -> Option<(&[Item], &Node)> {
    let [
        Item::Node(Node::Atom(head)),
        Item::Node(Node::List(bindings)),
        Item::Node(body),
    ] = items
    else {
        return None;
    };
    (head == "let").then_some((bindings.as_slice(), body))
}

fn map_pairs(items: &[Item]) -> Option<Vec<(&Node, &Node)>> {
    let nodes = items
        .iter()
        .map(|item| match item {
            Item::Node(node) => Some(node),
            Item::Comment(_) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let [Node::Atom(head), arguments @ ..] = nodes.as_slice() else {
        return None;
    };
    if head != "map" || arguments.len() % 2 != 0 {
        return None;
    }
    Some(
        arguments
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .collect(),
    )
}

fn render_map(pairs: Vec<(&Node, &Node)>, indent: usize, writer: &mut Writer) -> YanshuResult<()> {
    writer.write("(map")?;
    let pair_indent = indent + writer.options.indent_width;
    for (key, value) in pairs {
        writer.newline(pair_indent, false)?;
        render_node(key, pair_indent, writer)?;
        if let Some(length) = flat_length(value)
            && writer.column.saturating_add(1).saturating_add(length) <= writer.options.line_width
        {
            writer.write(" ")?;
            render_flat(value, writer)?;
        } else {
            writer.newline(pair_indent + writer.options.indent_width, false)?;
            render_node(value, pair_indent + writer.options.indent_width, writer)?;
        }
    }
    writer.write(")")
}

fn render_let(
    bindings: &[Item],
    body: &Node,
    indent: usize,
    writer: &mut Writer,
) -> YanshuResult<()> {
    writer.write("(let (")?;
    let binding_indent = writer.column;
    let mut first = true;
    let mut last_was_comment = false;
    for item in bindings {
        if !first {
            writer.newline(binding_indent, false)?;
        }
        match item {
            Item::Node(node) => {
                render_node(node, binding_indent, writer)?;
                last_was_comment = false;
            }
            Item::Comment(comment) => {
                writer.write(comment)?;
                last_was_comment = true;
            }
        }
        first = false;
    }
    if last_was_comment {
        writer.newline(indent + writer.options.indent_width, false)?;
    }
    writer.write(")")?;
    writer.newline(indent + writer.options.indent_width, false)?;
    render_node(body, indent + writer.options.indent_width, writer)?;
    writer.write(")")
}

fn preferred_prefix_nodes(head: Option<&str>) -> usize {
    match head {
        Some(
            "data" | "def" | "fn" | "if" | "match" | "optional" | "required" | "schema"
            | "signature",
        ) => 2,
        Some("route") => 4,
        Some(_) | None => 1,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramCategory {
    Header,
    Type,
    Route,
    Definition,
    Export,
    Other,
}

fn program_category(node: &Node) -> ProgramCategory {
    match node.head_symbol() {
        Some("name" | "version" | "imports" | "capabilities" | "libraries") => {
            ProgramCategory::Header
        }
        Some("data" | "schema" | "signature" | "export-types") => ProgramCategory::Type,
        Some("route") => ProgramCategory::Route,
        Some("def") => ProgramCategory::Definition,
        Some("export") => ProgramCategory::Export,
        Some(_) | None => ProgramCategory::Other,
    }
}

fn flat_length(node: &Node) -> Option<usize> {
    match node {
        Node::Atom(value) | Node::String(value) => {
            (!value.contains(['\n', '\r'])).then(|| value.chars().count())
        }
        Node::Quote { comments, value } if comments.is_empty() => {
            flat_length(value)?.checked_add(1)
        }
        Node::Quote { .. } => None,
        Node::List(items) => {
            let mut length = 2_usize;
            for (index, item) in items.iter().enumerate() {
                let Item::Node(node) = item else {
                    return None;
                };
                if index > 0 {
                    length = length.checked_add(1)?;
                }
                length = length.checked_add(flat_length(node)?)?;
            }
            Some(length)
        }
    }
}

fn render_flat(node: &Node, writer: &mut Writer) -> YanshuResult<()> {
    match node {
        Node::Atom(value) | Node::String(value) => writer.write(value),
        Node::Quote { comments, value } if comments.is_empty() => {
            writer.write("'")?;
            render_flat(value, writer)
        }
        Node::Quote { .. } => Err(Diagnostic::simple(
            "FORMAT_INTERNAL_LAYOUT",
            "commented quote cannot use flat layout",
        )),
        Node::List(items) => {
            writer.write("(")?;
            for (index, item) in items.iter().enumerate() {
                let Item::Node(node) = item else {
                    return Err(Diagnostic::simple(
                        "FORMAT_INTERNAL_LAYOUT",
                        "commented list cannot use flat layout",
                    ));
                };
                if index > 0 {
                    writer.write(" ")?;
                }
                render_flat(node, writer)?;
            }
            writer.write(")")
        }
    }
}

struct Writer {
    output: String,
    column: usize,
    options: FormatOptions,
}

impl Writer {
    fn new(options: FormatOptions) -> Self {
        Self {
            output: String::new(),
            column: 0,
            options,
        }
    }

    fn write(&mut self, value: &str) -> YanshuResult<()> {
        self.reserve(value.len())?;
        self.output.push_str(value);
        if let Some(last_line) = value.rsplit('\n').next() {
            if value.contains('\n') {
                self.column = last_line.chars().count();
            } else {
                self.column = self.column.saturating_add(last_line.chars().count());
            }
        }
        Ok(())
    }

    fn newline(&mut self, indent: usize, blank: bool) -> YanshuResult<()> {
        let newlines = if blank { 2 } else { 1 };
        let bytes = newlines + indent;
        self.reserve(bytes)?;
        for _ in 0..newlines {
            self.output.push('\n');
        }
        for _ in 0..indent {
            self.output.push(' ');
        }
        self.column = indent;
        Ok(())
    }

    fn reserve(&mut self, additional: usize) -> YanshuResult<()> {
        let requested = self.output.len().checked_add(additional).ok_or_else(|| {
            Diagnostic::simple(
                "FORMAT_OUTPUT_LIMIT",
                "formatter output size overflowed the host integer range",
            )
        })?;
        if requested > self.options.max_output_bytes {
            return Err(Diagnostic::new(
                "FORMAT_OUTPUT_LIMIT",
                "formatted source exceeds the configured byte limit",
                json!({
                    "maximum": self.options.max_output_bytes,
                    "requested": requested,
                }),
            ));
        }
        Ok(())
    }

    fn finish(mut self) -> YanshuResult<String> {
        if !self.output.ends_with('\n') {
            self.write("\n")?;
        }
        Ok(self.output)
    }
}
