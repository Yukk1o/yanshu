use yanshu_diagnostic::Span;
use yanshu_syntax::{Datum, DatumKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SymbolToken {
    pub(super) name: String,
    pub(super) span: Span,
    pub(super) parent_span: Span,
    pub(super) parent_depth: usize,
    pub(super) is_head: bool,
    pub(super) top_level_form: Option<String>,
}

pub(super) fn symbol_at(root: &Datum, offset: usize) -> Option<SymbolToken> {
    search(root, offset, 0, None)
}

fn search(
    datum: &Datum,
    offset: usize,
    list_depth: usize,
    top_level_form: Option<&str>,
) -> Option<SymbolToken> {
    let DatumKind::List(items) = &datum.kind else {
        return None;
    };
    for (index, item) in items.iter().enumerate() {
        if !contains_offset(item.span, offset) {
            continue;
        }
        if let DatumKind::Symbol(name) = &item.kind {
            return Some(SymbolToken {
                name: name.clone(),
                span: item.span,
                parent_span: datum.span,
                parent_depth: list_depth,
                is_head: index == 0,
                top_level_form: top_level_form.map(str::to_owned),
            });
        }
        let child_top_level_form = if list_depth == 0 {
            item.list()
                .and_then(|form| form.first())
                .and_then(Datum::symbol)
        } else {
            top_level_form
        };
        return search(item, offset, list_depth + 1, child_top_level_form);
    }
    None
}

fn contains_offset(span: Span, offset: usize) -> bool {
    span.start.offset <= offset && offset < span.end.offset
}

#[cfg(test)]
mod tests {
    use yanshu_syntax::{ReaderLimits, read_source};

    use super::symbol_at;

    #[test]
    fn reports_exact_symbol_context_without_reading_strings_or_comments() {
        let source = "(program (name sample) (def run (fn () (quote (cond log)))))";
        let root = read_source(source, ReaderLimits::default())
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

        let definition = source
            .find("def run")
            .unwrap_or_else(|| panic!("definition marker missing"));
        let token = symbol_at(&root, definition)
            .unwrap_or_else(|| panic!("definition keyword token missing"));
        assert_eq!(token.name, "def");
        assert_eq!(token.parent_depth, 1);
        assert_eq!(token.top_level_form.as_deref(), Some("def"));
        assert!(token.is_head);

        let quoted = source
            .find("cond log")
            .unwrap_or_else(|| panic!("quoted token missing"));
        let token =
            symbol_at(&root, quoted).unwrap_or_else(|| panic!("quoted symbol token missing"));
        assert_eq!(token.name, "cond");
        assert!(token.is_head);
        assert_eq!(token.top_level_form.as_deref(), Some("def"));
    }
}
