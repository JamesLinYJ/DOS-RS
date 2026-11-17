//! Quote recognition and list parsing precede expansion and redirection.
use alloc::{string::String, vec::Vec};
#[path = "grammar.rs"]
mod grammar;
pub use grammar::parse;

#[derive(PartialEq, Debug)]
pub enum Part {
    Literal(String),
    Parameter(String, bool),
}
pub type Word = Vec<Part>;
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Read,
    Write,
    Append,
}
pub enum Compound {
    Function {
        name: String,
        body: alloc::rc::Rc<Command>,
    },
    Group(Vec<Pipeline>),
    Subshell(Vec<Pipeline>),
    If {
        arms: Vec<(Vec<Pipeline>, Vec<Pipeline>)>,
        otherwise: Vec<Pipeline>,
    },
    While {
        until: bool,
        test: Vec<Pipeline>,
        body: Vec<Pipeline>,
    },
    For {
        name: String,
        words: Option<Vec<Word>>,
        body: Vec<Pipeline>,
    },
}

#[derive(Debug)]
pub enum Error {
    Incomplete(&'static str),
    Invalid(&'static str),
}
impl Error {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Incomplete(s) | Self::Invalid(s) => s,
        }
    }
}

pub struct Redirect {
    pub fd: i32,
    pub mode: Mode,
    pub target: Word,
}
#[derive(Default)]
pub struct Command {
    pub compound: Option<Compound>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Condition {
    Always,
    Success,
    Failure,
}
pub struct Pipeline {
    pub commands: Vec<Command>,
    pub when: Condition,
    pub background: bool,
    pub negate: bool,
}

enum Token {
    Word(Word, bool),
    Newline,
    LeftParen,
    RightParen,
    Redirect(i32, Mode),
    Pipe,
    End(Condition),
    Background,
}

fn literal(word: &mut Word, ch: char) {
    if let Some(Part::Literal(s)) = word.last_mut() {
        s.push(ch);
    } else {
        word.push(Part::Literal(String::from(ch)));
    }
}
pub fn identifier(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|c| c == b'_' || c.is_ascii_alphabetic())
        && bytes.all(|c| c == b'_' || c.is_ascii_alphanumeric())
}

fn word(input: &mut core::iter::Peekable<core::str::Chars<'_>>) -> Result<(Word, bool), Error> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut quote = None;
    while let Some(&c) = input.peek() {
        if quote.is_none() && (c.is_ascii_whitespace() || "|&;<>()".contains(c)) {
            break;
        }
        input.next();
        match c {
            '\'' | '"' if quote == Some(c) => quote = None,
            '\'' | '"' if quote.is_none() => {
                quoted = true;
                quote = Some(c);
                if input.peek() == Some(&c) && !matches!(parts.last(), Some(Part::Literal(_))) {
                    parts.push(Part::Literal(String::new()));
                }
            }
            '\\' if quote != Some('\'') => {
                quoted = true;
                let escaped = input.next().ok_or(Error::Incomplete("unfinished escape"))?;
                if quote == Some('"') && !"$`\"\\\n".contains(escaped) {
                    literal(&mut parts, '\\');
                }
                if escaped != '\n' {
                    literal(&mut parts, escaped);
                }
            }
            '$' if quote != Some('\'') => {
                quoted = true;
                let mut name = String::new();
                if input.next_if_eq(&'{').is_some() {
                    loop {
                        match input.next() {
                            Some('}') => break,
                            Some(ch) => name.push(ch),
                            None => return Err(Error::Incomplete("unterminated parameter")),
                        }
                    }
                } else if let Some(&ch) = input.peek() {
                    if ch == '_' || ch.is_ascii_alphabetic() {
                        while let Some(ch) =
                            input.next_if(|v| *v == '_' || v.is_ascii_alphanumeric())
                        {
                            name.push(ch);
                        }
                    } else if "?$#!@*".contains(ch) || ch.is_ascii_digit() {
                        name.push(input.next().unwrap());
                    }
                }
                if name.is_empty() {
                    literal(&mut parts, '$');
                } else {
                    parts.push(Part::Parameter(name, quote.is_some()));
                }
            }
            _ => literal(&mut parts, c),
        }
    }
    if quote.is_some() {
        return Err(Error::Incomplete("unterminated quote"));
    }
    Ok((parts, quoted))
}

fn tokens(line: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            '\n' => {
                it.next();
                tokens.push(Token::Newline);
            }
            '(' => {
                it.next();
                tokens.push(Token::LeftParen);
            }
            ')' => {
                it.next();
                tokens.push(Token::RightParen);
            }
            ';' => {
                it.next();
                tokens.push(Token::End(Condition::Always));
            }
            c if c.is_ascii_whitespace() => {
                it.next();
            }
            '#' => while it.next_if(|v| *v != '\n').is_some() {},
            '|' => {
                it.next();
                tokens.push(if it.next_if_eq(&'|').is_some() {
                    Token::End(Condition::Failure)
                } else {
                    Token::Pipe
                });
            }
            '&' => {
                it.next();
                tokens.push(if it.next_if_eq(&'&').is_some() {
                    Token::End(Condition::Success)
                } else {
                    Token::Background
                });
            }
            _ => {
                let mut digits = it.clone();
                let numeral: String =
                    core::iter::from_fn(|| digits.next_if(char::is_ascii_digit)).collect();
                if let Some(op) = digits.next_if(|v| *v == '<' || *v == '>') {
                    let fd = if numeral.is_empty() {
                        i32::from(op == '>')
                    } else {
                        numeral
                            .parse()
                            .map_err(|_| Error::Invalid("invalid redirection descriptor"))?
                    };
                    let mode = if op == '<' {
                        Mode::Read
                    } else if digits.next_if_eq(&'>').is_some() {
                        Mode::Append
                    } else {
                        Mode::Write
                    };
                    it = digits;
                    tokens.push(Token::Redirect(fd, mode));
                } else {
                    let (parts, quoted) = word(&mut it)?;
                    tokens.push(Token::Word(parts, quoted));
                }
            }
        }
    }
    Ok(tokens)
}
