use yanshu_diagnostic::{Diagnostic, YanshuResult};

use super::{MAXIMUM_SEMANTIC_TOKENS, SEMANTIC_TOKEN_INTEGER_COUNT, SemanticToken, token_limit};

pub(super) fn encode_tokens(source: &str, tokens: &[SemanticToken]) -> YanshuResult<Vec<u32>> {
    if tokens.len() > MAXIMUM_SEMANTIC_TOKENS {
        return Err(token_limit(tokens.len()));
    }
    let capacity = tokens
        .len()
        .checked_mul(SEMANTIC_TOKEN_INTEGER_COUNT)
        .ok_or_else(encoding_failure)?;
    let mut data = Vec::with_capacity(capacity);
    let mut cursor = PositionCursor::default();
    let mut previous_line = 0_u32;
    let mut previous_start = 0_u32;

    for token in tokens {
        let (line, start) = cursor
            .advance_to(source, token.span.start.offset)
            .ok_or_else(encoding_failure)?;
        let (end_line, end) = cursor
            .advance_to(source, token.span.end.offset)
            .ok_or_else(encoding_failure)?;
        if line != end_line || end <= start || line < previous_line {
            return Err(encoding_failure());
        }
        let delta_line = line - previous_line;
        let delta_start = if delta_line == 0 {
            start
                .checked_sub(previous_start)
                .ok_or_else(encoding_failure)?
        } else {
            start
        };
        data.extend_from_slice(&[
            delta_line,
            delta_start,
            end - start,
            token.token_type as u32,
            token.modifiers,
        ]);
        previous_line = line;
        previous_start = start;
    }
    Ok(data)
}

#[derive(Default)]
struct PositionCursor {
    offset: usize,
    line: u32,
    character: u32,
}

impl PositionCursor {
    fn advance_to(&mut self, source: &str, target: usize) -> Option<(u32, u32)> {
        if target < self.offset {
            return None;
        }
        for character in source.get(self.offset..target)?.chars() {
            if character == '\n' {
                self.line = self.line.checked_add(1)?;
                self.character = 0;
            } else {
                self.character = self
                    .character
                    .checked_add(u32::try_from(character.len_utf16()).ok()?)?;
            }
        }
        self.offset = target;
        Some((self.line, self.character))
    }
}

fn encoding_failure() -> Diagnostic {
    Diagnostic::simple(
        "LSP_SEMANTIC_TOKEN_ENCODING",
        "semantic tokens could not be encoded as ordered UTF-16 ranges",
    )
}
