use serde_json::json;
use yanshu_diagnostic::{Diagnostic, YanshuResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Document {
    pub(crate) leading_comments: Vec<String>,
    pub(crate) root: Node,
    pub(crate) trailing_comments: Vec<String>,
}

impl Document {
    pub(crate) fn comments(&self) -> Vec<&str> {
        let mut comments = Vec::new();
        comments.extend(self.leading_comments.iter().map(String::as_str));
        self.root.collect_comments(&mut comments);
        comments.extend(self.trailing_comments.iter().map(String::as_str));
        comments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Atom(String),
    String(String),
    Quote {
        comments: Vec<String>,
        value: Box<Node>,
    },
    List(Vec<Item>),
}

impl Node {
    fn collect_comments<'node>(&'node self, comments: &mut Vec<&'node str>) {
        match self {
            Self::Atom(_) | Self::String(_) => {}
            Self::Quote {
                comments: quote_comments,
                value,
            } => {
                comments.extend(quote_comments.iter().map(String::as_str));
                value.collect_comments(comments);
            }
            Self::List(items) => {
                for item in items {
                    match item {
                        Item::Comment(comment) => comments.push(comment),
                        Item::Node(node) => node.collect_comments(comments),
                    }
                }
            }
        }
    }

    pub(crate) fn head_symbol(&self) -> Option<&str> {
        let Self::List(items) = self else {
            return None;
        };
        first_node(items).and_then(|node| match node {
            Self::Atom(value) => Some(value.as_str()),
            Self::String(_) | Self::Quote { .. } | Self::List(_) => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Node(Node),
    Comment(String),
}

pub(crate) struct ConcreteParser<'source> {
    source: &'source str,
    offset: usize,
}

impl<'source> ConcreteParser<'source> {
    pub(crate) fn new(source: &'source str) -> Self {
        Self { source, offset: 0 }
    }

    pub(crate) fn parse_document(mut self) -> YanshuResult<Document> {
        let leading_comments = self.comments_before_node();
        let root = self.read_node()?;
        let trailing_comments = self.comments_before_node();
        if self.peek().is_some() {
            return Err(self.internal("formatter concrete parser found trailing source"));
        }
        Ok(Document {
            leading_comments,
            root,
            trailing_comments,
        })
    }

    fn comments_before_node(&mut self) -> Vec<String> {
        let mut comments = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek() != Some(';') {
                return comments;
            }
            comments.push(self.read_comment());
        }
    }

    fn read_node(&mut self) -> YanshuResult<Node> {
        self.skip_whitespace();
        match self.peek() {
            Some('(') => self.read_list('(', ')'),
            Some('[') => self.read_list('[', ']'),
            Some('{') => self.read_list('{', '}'),
            Some('\'') => self.read_quote(),
            Some('"') => self.read_string(),
            Some(_) => self.read_atom(),
            None => Err(self.internal("formatter concrete parser expected a datum")),
        }
    }

    fn read_list(&mut self, open: char, close: char) -> YanshuResult<Node> {
        if self.bump() != Some(open) {
            return Err(self.internal("formatter concrete parser lost an opening delimiter"));
        }
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(character) if character == close => {
                    let _close = self.bump();
                    return Ok(Node::List(items));
                }
                Some(';') => items.push(Item::Comment(self.read_comment())),
                Some(_) => items.push(Item::Node(self.read_node()?)),
                None => {
                    return Err(
                        self.internal("formatter concrete parser found an unterminated list")
                    );
                }
            }
        }
    }

    fn read_quote(&mut self) -> YanshuResult<Node> {
        let _quote = self.bump();
        let comments = self.comments_before_node();
        let value = Box::new(self.read_node()?);
        Ok(Node::Quote { comments, value })
    }

    fn read_string(&mut self) -> YanshuResult<Node> {
        let start = self.offset;
        let _opening = self.bump();
        loop {
            match self.bump() {
                Some('"') => {
                    let raw = self.source.get(start..self.offset).ok_or_else(|| {
                        self.internal("formatter string was not on a UTF-8 boundary")
                    })?;
                    return Ok(Node::String(raw.to_owned()));
                }
                Some('\\') => {
                    if self.bump().is_none() {
                        return Err(self.internal("formatter found an unterminated string escape"));
                    }
                }
                Some(_) => {}
                None => return Err(self.internal("formatter found an unterminated string")),
            }
        }
    }

    fn read_atom(&mut self) -> YanshuResult<Node> {
        let start = self.offset;
        while self.peek().is_some_and(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ';'
                )
        }) {
            let _character = self.bump();
        }
        let raw = self
            .source
            .get(start..self.offset)
            .ok_or_else(|| self.internal("formatter atom was not on a UTF-8 boundary"))?;
        if raw.is_empty() {
            return Err(self.internal("formatter concrete parser found an empty atom"));
        }
        Ok(Node::Atom(raw.to_owned()))
    }

    fn read_comment(&mut self) -> String {
        let start = self.offset;
        while self.peek().is_some_and(|character| character != '\n') {
            let _character = self.bump();
        }
        self.source
            .get(start..self.offset)
            .unwrap_or_default()
            .trim_end_matches(['\r', ' ', '\t'])
            .to_owned()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            let _character = self.bump();
        }
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.offset..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        Some(character)
    }

    fn internal(&self, message: &str) -> Diagnostic {
        Diagnostic::new(
            "FORMAT_INTERNAL_CST",
            message,
            json!({ "offset": self.offset }),
        )
    }
}

pub(crate) fn first_node(items: &[Item]) -> Option<&Node> {
    items.iter().find_map(|item| match item {
        Item::Node(node) => Some(node),
        Item::Comment(_) => None,
    })
}
