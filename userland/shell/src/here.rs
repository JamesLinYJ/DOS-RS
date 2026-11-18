//! Read bodies in source order, then expand them only when a redirection executes.
use super::{Error, Input, Part, Word, backquote, dollar, literal};
use alloc::{string::String, vec::Vec};
use core::cell::OnceCell;

pub struct HereDocument {
    delimiter: String,
    quoted: bool,
    strip_tabs: bool,
    body: OnceCell<String>,
}

impl HereDocument {
    pub(super) fn delimiter(
        input: &mut Input<'_>,
        strip_tabs: bool,
        depth: u16,
    ) -> Result<Self, Error> {
        while input.next_if(|c| matches!(c, ' ' | '\t')).is_some() {}
        let mut delimiter = String::new();
        let mut quoted = false;
        let mut started = false;
        let mut quote = None;
        while let Some(&c) = input.peek() {
            if quote.is_none()
                && (c.is_ascii_whitespace() || "|&;<>()".contains(c) || !started && c == '#')
            {
                break;
            }
            input.next();
            match c {
                '\'' | '"' if quote == Some(c) => quote = None,
                '\'' | '"' if quote.is_none() => {
                    quoted = true;
                    quote = Some(c);
                }
                '\\' if quote != Some('\'') => {
                    let escaped = input
                        .next()
                        .ok_or(Error::Incomplete("unfinished here-document delimiter"))?;
                    if escaped == '\n' {
                        continue;
                    }
                    quoted = true;
                    if quote == Some('"') && !"$`\"\\".contains(escaped) {
                        delimiter.push('\\');
                    }
                    delimiter.push(escaped);
                }
                '$' | '`'
                    if quote != Some('\'')
                        && (c == '`' || matches!(input.peek(), Some('{' | '('))) =>
                {
                    // Nested shell syntax is part of the delimiter's spelling.
                    // Scan it using the same grammar as an ordinary shell word.
                    let mut end = input.clone();
                    if c == '$' {
                        dollar(&mut Vec::new(), &mut end, depth, quote.is_some())?;
                    } else {
                        backquote(&mut end, depth, quote.is_some())?;
                    }
                    let consumed = input.clone().count() - end.count();
                    delimiter.push(c);
                    delimiter.extend(input.by_ref().take(consumed));
                }
                _ => delimiter.push(c),
            }
            started = true;
        }
        if quote.is_some() {
            return Err(Error::Incomplete("unterminated quoted delimiter"));
        }
        if !started {
            return Err(if input.peek().is_none() {
                Error::Incomplete("expected here-document delimiter")
            } else {
                Error::Invalid("expected here-document delimiter")
            });
        }
        Ok(Self {
            delimiter,
            quoted,
            strip_tabs,
            body: OnceCell::new(),
        })
    }

    pub(super) fn read(&self, input: &mut Input<'_>) -> Result<(), Error> {
        let mut body = String::new();
        let mut line = String::new();
        loop {
            match input.next() {
                Some('\\') if !self.quoted => match input.peek() {
                    Some('\n') => {
                        input.next();
                    }
                    Some('\\') => {
                        input.next();
                        line.push_str("\\\\");
                    }
                    _ => line.push('\\'),
                },
                Some(c) if c != '\n' => line.push(c),
                end => {
                    let text = if self.strip_tabs {
                        line.trim_start_matches('\t')
                    } else {
                        &line
                    };
                    if text == self.delimiter {
                        return self
                            .body
                            .set(body)
                            .map_err(|_| Error::Invalid("duplicate here-document body"));
                    }
                    if end.is_none() {
                        return Err(Error::Incomplete("unterminated here-document"));
                    }
                    body.push_str(text);
                    body.push('\n');
                    line.clear();
                }
            }
        }
    }

    pub fn word(&self) -> Result<Word, Error> {
        let body = self
            .body
            .get()
            .ok_or(Error::Incomplete("missing here-document body"))?;
        if self.quoted {
            return Ok(alloc::vec![Part::Quoted(body.clone())]);
        }
        let mut word = Vec::new();
        let mut input = body.chars().peekable();
        while let Some(c) = input.next() {
            match c {
                '$' => dollar(&mut word, &mut input, 0, true)?,
                '`' => word.push(Part::Substitution(backquote(&mut input, 0, false)?, true)),
                '\\' => match input.next_if(|v| matches!(v, '$' | '`' | '\\')) {
                    Some(escaped) => literal(&mut word, escaped, true),
                    None => literal(&mut word, c, true),
                },
                _ => literal(&mut word, c, true),
            }
        }
        Ok(word)
    }
}
