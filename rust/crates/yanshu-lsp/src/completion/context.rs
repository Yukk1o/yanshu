use yanshu_syntax::{Datum, DatumKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SiteKind {
    Expression { head: bool },
    TopLevel { root: bool },
    Type { head: bool },
    Schema { head: bool },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletionSite {
    pub(super) prefix: String,
    pub(super) replace_start: usize,
    pub(super) replace_end: usize,
    pub(super) kind: SiteKind,
}

pub(super) fn completion_site(root: &Datum, source: &str, offset: usize) -> Option<CompletionSite> {
    if offset > source.len() || !source.is_char_boundary(offset) || in_comment(source, offset) {
        return None;
    }
    let raw = locate_list(root, source, offset, 0, None, false)?;
    let kind = if raw.is_head && raw.parent_depth <= 1 {
        SiteKind::TopLevel {
            root: raw.parent_depth == 0,
        }
    } else {
        match raw.top_level_form.as_deref() {
            Some("def") if raw.parent_depth > 1 || raw.item_index >= 2 => {
                SiteKind::Expression { head: raw.is_head }
            }
            Some("signature") if raw.parent_depth > 1 || raw.item_index >= 2 => {
                SiteKind::Type { head: raw.is_head }
            }
            Some("data") if raw.parent_depth >= 3 && raw.item_index >= 1 => {
                SiteKind::Type { head: raw.is_head }
            }
            Some("schema") if raw.parent_depth > 1 || raw.item_index >= 2 => {
                SiteKind::Schema { head: raw.is_head }
            }
            _ => SiteKind::Other,
        }
    };
    Some(CompletionSite {
        prefix: raw.prefix,
        replace_start: raw.replace_start,
        replace_end: raw.replace_end,
        kind,
    })
}

struct RawSite {
    prefix: String,
    replace_start: usize,
    replace_end: usize,
    parent_depth: usize,
    item_index: usize,
    is_head: bool,
    top_level_form: Option<String>,
}

fn locate_list(
    datum: &Datum,
    source: &str,
    offset: usize,
    list_depth: usize,
    top_level_form: Option<&str>,
    quoted: bool,
) -> Option<RawSite> {
    let DatumKind::List(items) = &datum.kind else {
        return None;
    };
    let head = items.first().and_then(Datum::symbol);
    for (index, item) in items.iter().enumerate() {
        let quoted_item = quoted || (head == Some("quote") && index > 0);
        if let DatumKind::Symbol(_) = &item.kind
            && item.span.start.offset <= offset
            && offset <= item.span.end.offset
        {
            if quoted_item {
                return None;
            }
            let prefix = source.get(item.span.start.offset..offset)?.to_owned();
            return Some(RawSite {
                prefix,
                replace_start: item.span.start.offset,
                replace_end: item.span.end.offset,
                parent_depth: list_depth,
                item_index: index,
                is_head: index == 0,
                top_level_form: top_level_form.map(str::to_owned),
            });
        }
        if item.span.start.offset < offset && offset < item.span.end.offset {
            if quoted_item {
                return None;
            }
            let child_top_level = if list_depth == 0 {
                item.list()
                    .and_then(|form| form.first())
                    .and_then(Datum::symbol)
            } else {
                top_level_form
            };
            return locate_list(
                item,
                source,
                offset,
                list_depth + 1,
                child_top_level,
                quoted_item,
            );
        }
    }

    if quoted
        || head == Some("quote")
            && items
                .first()
                .is_some_and(|item| offset > item.span.end.offset)
    {
        return None;
    }
    if offset <= datum.span.start.offset || offset >= datum.span.end.offset {
        return None;
    }
    let item_index = items
        .iter()
        .take_while(|item| item.span.end.offset <= offset)
        .count();
    Some(RawSite {
        prefix: String::new(),
        replace_start: offset,
        replace_end: offset,
        parent_depth: list_depth,
        item_index,
        is_head: item_index == 0,
        top_level_form: top_level_form.map(str::to_owned),
    })
}

fn in_comment(source: &str, offset: usize) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        } else if character == ';' {
            in_comment = true;
        }
    }
    in_comment || in_string
}

#[cfg(test)]
mod tests {
    use yanshu_syntax::{ReaderLimits, read_source};

    use super::{SiteKind, completion_site};

    #[test]
    fn classifies_reader_context_and_excludes_data_and_comments() {
        let source = r#"(program
  (name completion)
  (version 4)
  (schema action (enum "approve" "reject"))
  (signature run (fn (integer) integer))
  ; log is comment data
  (def run (fn (value) (cond ((> value 0) value) (else '(cond log)))))
  (export run))"#;
        let root = read_source(source, ReaderLimits::default())
            .unwrap_or_else(|diagnostic| panic!("{diagnostic}"));

        let top = source
            .find("signature")
            .unwrap_or_else(|| panic!("signature missing"));
        assert_eq!(
            completion_site(&root, source, top).map(|site| site.kind),
            Some(SiteKind::TopLevel { root: false })
        );

        let type_form = source
            .find("(fn (integer)")
            .unwrap_or_else(|| panic!("type form missing"))
            + 1;
        assert_eq!(
            completion_site(&root, source, type_form).map(|site| site.kind),
            Some(SiteKind::Type { head: true })
        );

        let schema = source
            .find("enum \"approve\"")
            .unwrap_or_else(|| panic!("schema form missing"));
        assert_eq!(
            completion_site(&root, source, schema).map(|site| site.kind),
            Some(SiteKind::Schema { head: true })
        );

        let cond = source
            .find("cond ((>")
            .unwrap_or_else(|| panic!("cond missing"));
        let partial = completion_site(&root, source, cond + 2)
            .unwrap_or_else(|| panic!("expression site missing"));
        assert_eq!(partial.kind, SiteKind::Expression { head: true });
        assert_eq!(partial.prefix, "co");
        assert_eq!(&source[partial.replace_start..partial.replace_end], "cond");

        let quoted = source
            .rfind("cond log")
            .unwrap_or_else(|| panic!("quoted data missing"));
        assert_eq!(completion_site(&root, source, quoted), None);
        let comment = source
            .find("log is comment")
            .unwrap_or_else(|| panic!("comment missing"));
        assert_eq!(completion_site(&root, source, comment), None);
    }
}
