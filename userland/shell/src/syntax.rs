//! Quote recognition and list parsing precede expansion and redirection.
use alloc::{string::String, vec::Vec};

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
pub struct Redirect {
    pub fd: i32,
    pub mode: Mode,
    pub target: Word,
}
#[derive(Default)]
pub struct Command {
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
}

enum Token {
    Word(Word),
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

fn word(input: &mut core::iter::Peekable<core::str::Chars<'_>>) -> Result<Word, &'static str> {
    let mut parts = Vec::new();
    let mut quote = None;
    while let Some(&c) = input.peek() {
        if quote.is_none() && (c.is_ascii_whitespace() || "|&;<>".contains(c)) {
            break;
        }
        input.next();
        match c {
            '\'' | '"' if quote == Some(c) => quote = None,
            '\'' | '"' if quote.is_none() => {
                quote = Some(c);
                if !matches!(parts.last(), Some(Part::Literal(_))) {
                    parts.push(Part::Literal(String::new()));
                }
            }
            '\\' if quote != Some('\'') => {
                let escaped = input.next().ok_or("unfinished escape")?;
                if quote == Some('"') && !"$`\"\\\n".contains(escaped) {
                    literal(&mut parts, '\\');
                }
                if escaped != '\n' {
                    literal(&mut parts, escaped);
                }
            }
            '$' if quote != Some('\'') => {
                let mut name = String::new();
                if input.next_if_eq(&'{').is_some() {
                    loop {
                        match input.next() {
                            Some('}') => break,
                            Some(ch) => name.push(ch),
                            None => return Err("unterminated parameter"),
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
        return Err("unterminated quote");
    }
    Ok(parts)
}

fn tokens(line: &str) -> Result<Vec<Token>, &'static str> {
    let mut tokens = Vec::new();
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            '\n' | ';' => {
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
                            .map_err(|_| "invalid redirection descriptor")?
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
                    tokens.push(Token::Word(word(&mut it)?));
                }
            }
        }
    }
    Ok(tokens)
}

pub fn parse(line: &str) -> Result<Vec<Pipeline>, &'static str> {
    let mut list = Vec::new();
    let mut commands = Vec::new();
    let mut cmd = Command::default();
    let mut when = Condition::Always;
    let mut tokens = tokens(line)?.into_iter().peekable();
    while let Some(t) = tokens.next() {
        match t {
            Token::Word(w) => cmd.words.push(w),
            Token::Redirect(fd, mode) => {
                let Some(Token::Word(target)) = tokens.next() else {
                    return Err("expected redirection path");
                };
                cmd.redirects.push(Redirect { fd, mode, target });
            }
            t => {
                if cmd.words.is_empty() && cmd.redirects.is_empty() {
                    if commands.is_empty()
                        && when == Condition::Always
                        && matches!(t, Token::End(Condition::Always))
                    {
                        continue;
                    }
                    return Err("expected command");
                }
                commands.push(core::mem::take(&mut cmd));
                let background = matches!(t, Token::Background);
                let next = match t {
                    Token::Pipe => {
                        if tokens.peek().is_none() {
                            return Err("unfinished pipeline");
                        }
                        continue;
                    }
                    Token::End(cond) => cond,
                    Token::Background => Condition::Always,
                    _ => unreachable!(),
                };
                list.push(Pipeline {
                    commands: core::mem::take(&mut commands),
                    when,
                    background,
                });
                when = next;
            }
        }
    }
    if !cmd.words.is_empty() || !cmd.redirects.is_empty() {
        commands.push(cmd);
        list.push(Pipeline {
            commands,
            when,
            background: false,
        });
    } else if when != Condition::Always {
        return Err("missing conditional command");
    }
    Ok(list)
}
