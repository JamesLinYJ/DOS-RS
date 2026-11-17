//! Quote recognition and list parsing precede expansion and redirection.
use alloc::rc::Rc;
use alloc::{string::String, vec::Vec};
#[path = "here.rs"]
mod here;
pub use here::HereDocument;
#[path = "grammar.rs"]
mod grammar;
pub use grammar::parse;

pub enum Part {
    Literal(String),
    Quoted(String),
    Parameter(String, bool),
    Braced(String, Modifier, bool),
    Substitution(Vec<Pipeline>, bool),
    Arithmetic(Word, bool),
}
pub enum Modifier {
    Length,
    Select {
        op: char,
        colon: bool,
        word: Word,
    },
    Trim {
        prefix: bool,
        longest: bool,
        pattern: Word,
    },
}
pub type Word = Vec<Part>;
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Duplicate,
    ReadWrite,
    Read,
    Write,
    Append,
}
pub struct CaseArm {
    pub patterns: Vec<Word>,
    pub body: Vec<Pipeline>,
    pub fall_through: bool,
}

pub enum Compound {
    Case {
        value: Word,
        arms: Vec<CaseArm>,
    },
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
    pub target: Target,
}
pub enum Target {
    File(Mode, Word),
    Here(Rc<HereDocument>),
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
    CaseEnd(bool),
    Newline,
    LeftParen,
    RightParen,
    Redirect(i32, Mode),
    Here(i32, Rc<HereDocument>),
    Pipe,
    End(Condition),
    Background,
}

fn literal(word: &mut Word, ch: char, quoted: bool) {
    match (word.last_mut(), quoted) {
        (Some(Part::Literal(text)), false) | (Some(Part::Quoted(text)), true) => text.push(ch),
        (_, true) => word.push(Part::Quoted(String::from(ch))),
        (_, false) => word.push(Part::Literal(String::from(ch))),
    }
}
pub fn identifier(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|c| c == b'_' || c.is_ascii_alphabetic())
        && bytes.all(|c| c == b'_' || c.is_ascii_alphanumeric())
}

type Input<'s> = core::iter::Peekable<core::str::Chars<'s>>;

fn backquote(
    input: &mut Input<'_>,
    depth: u16,
    in_double_quote: bool,
) -> Result<Vec<Pipeline>, Error> {
    let mut source = String::new();
    loop {
        match input.next() {
            Some('`') => break,
            Some('\\') => {
                let ch = input
                    .next()
                    .ok_or(Error::Incomplete("unfinished backquote"))?;
                if !matches!(ch, '$' | '`' | '\\' | '\n') && !(in_double_quote && ch == '"') {
                    source.push('\\');
                }
                if ch != '\n' {
                    source.push(ch);
                }
            }
            Some(c) => source.push(c),
            None => return Err(Error::Incomplete("unterminated backquote")),
        }
    }
    grammar::parse_input(&mut source.chars().peekable(), depth + 1)
}

fn dollar(parts: &mut Word, input: &mut Input<'_>, depth: u16, quoted: bool) -> Result<(), Error> {
    if depth >= 64 {
        return Err(Error::Invalid("expansion nested too deeply"));
    }
    if input.next_if_eq(&'(').is_some() {
        let mut nested = input.clone();
        if nested.next_if_eq(&'(').is_some()
            && let Some(word) = arithmetic_word(&mut nested, depth + 1)?
        {
            *input = nested;
            parts.push(Part::Arithmetic(word, quoted));
            return Ok(());
        }
        parts.push(Part::Substitution(
            grammar::substitution(input, depth + 1)?,
            quoted,
        ));
        return Ok(());
    }
    if input.next_if_eq(&'{').is_some() {
        parts.push(braced(input, depth + 1, quoted)?);
        return Ok(());
    }
    let name = parameter_name(input, false);
    if name.is_empty() {
        literal(parts, '$', quoted);
    } else {
        parts.push(Part::Parameter(name, quoted));
    }
    Ok(())
}

fn parameter_name(input: &mut Input<'_>, braced: bool) -> String {
    let mut name = String::new();
    if let Some(&ch) = input.peek() {
        if ch == '_' || ch.is_ascii_alphabetic() {
            while let Some(ch) = input.next_if(|v| *v == '_' || v.is_ascii_alphanumeric()) {
                name.push(ch);
            }
        } else if braced && ch.is_ascii_digit() {
            while let Some(ch) = input.next_if(char::is_ascii_digit) {
                name.push(ch);
            }
        } else if "?$#!@*-".contains(ch) || ch.is_ascii_digit() {
            name.push(input.next().unwrap());
        }
    }
    name
}

fn braced(input: &mut Input<'_>, depth: u16, quoted: bool) -> Result<Part, Error> {
    let mut length = input.clone();
    if length.next_if_eq(&'#').is_some() {
        let name = parameter_name(&mut length, true);
        if !name.is_empty() && length.next_if_eq(&'}').is_some() {
            *input = length;
            return Ok(Part::Braced(name, Modifier::Length, quoted));
        }
    }
    if input.peek().is_none() {
        return Err(Error::Incomplete("unterminated parameter"));
    }
    let name = parameter_name(input, true);
    if name.is_empty() {
        return Err(Error::Invalid("invalid parameter name"));
    }
    if input.next_if_eq(&'}').is_some() {
        return Ok(Part::Parameter(name, quoted));
    }
    let colon = input.next_if_eq(&':').is_some();
    let op = input
        .next()
        .ok_or(Error::Incomplete("unterminated parameter"))?;
    let modifier = match op {
        '-' | '+' | '=' | '?' => Modifier::Select {
            op,
            colon,
            word: word_parts(input, depth, true, quoted)?.0,
        },
        '%' | '#' if !colon => Modifier::Trim {
            prefix: op == '#',
            longest: input.next_if_eq(&op).is_some(),
            pattern: word_parts(input, depth, true, false)?.0,
        },
        _ => return Err(Error::Invalid("invalid parameter expansion")),
    };
    Ok(Part::Braced(name, modifier, quoted))
}

fn arithmetic_word(input: &mut Input<'_>, depth: u16) -> Result<Option<Word>, Error> {
    let mut word = Vec::new();
    let mut groups = 0usize;
    while let Some(ch) = input.next() {
        match ch {
            '(' => {
                groups += 1;
                literal(&mut word, ch, true);
            }
            ')' if groups == 0 => return Ok(input.next_if_eq(&')').map(|_| word)),
            ')' => {
                groups -= 1;
                literal(&mut word, ch, true);
            }
            '$' => dollar(&mut word, input, depth, true)?,
            '`' => word.push(Part::Substitution(backquote(input, depth, true)?, true)),
            '\\' => {
                let escaped = input
                    .next()
                    .ok_or(Error::Incomplete("unfinished arithmetic escape"))?;
                if !"$`\"\\\n".contains(escaped) {
                    literal(&mut word, ch, true);
                }
                if escaped != '\n' {
                    literal(&mut word, escaped, true);
                }
            }
            _ => literal(&mut word, ch, true),
        }
    }
    Err(Error::Incomplete("unterminated arithmetic expansion"))
}

fn word_parts(
    input: &mut Input<'_>,
    depth: u16,
    braced: bool,
    double: bool,
) -> Result<(Word, bool), Error> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut quote = None;
    let mut closed = false;
    while let Some(&c) = input.peek() {
        if braced && quote.is_none() && c == '}' {
            input.next();
            closed = true;
            break;
        }
        if !braced && quote.is_none() && (c.is_ascii_whitespace() || "|&;<>()".contains(c)) {
            break;
        }
        input.next();
        let protected = double || quote.is_some();
        match c {
            '\'' | '"' if quote == Some(c) => quote = None,
            '\'' | '"' if quote.is_none() && (!double || c == '"') => {
                quoted = true;
                quote = Some(c);
                if input.peek() == Some(&c) && !matches!(parts.last(), Some(Part::Quoted(_))) {
                    parts.push(Part::Quoted(String::new()));
                }
            }
            '\\' if quote != Some('\'') => {
                quoted = true;
                let escaped = input.next().ok_or(Error::Incomplete("unfinished escape"))?;
                if (double || quote == Some('"'))
                    && !(braced && escaped == '}')
                    && !"$`\"\\\n".contains(escaped)
                {
                    literal(&mut parts, '\\', true);
                }
                if escaped != '\n' {
                    literal(&mut parts, escaped, true);
                }
            }
            '`' if quote != Some('\'') => {
                quoted = true;
                parts.push(Part::Substitution(
                    backquote(input, depth, double || quote == Some('"'))?,
                    protected,
                ));
            }
            '$' if quote != Some('\'') => {
                quoted = true;
                dollar(&mut parts, input, depth, protected)?;
            }
            _ => literal(&mut parts, c, protected),
        }
    }
    if quote.is_some() {
        return Err(Error::Incomplete("unterminated quote"));
    }
    if braced && !closed {
        return Err(Error::Incomplete("unterminated parameter"));
    }
    Ok((parts, quoted))
}

fn token(it: &mut Input<'_>, depth: u16) -> Result<Option<Token>, Error> {
    while let Some(&c) = it.peek() {
        let token = match c {
            '\n' => {
                it.next();
                Token::Newline
            }
            '(' => {
                it.next();
                Token::LeftParen
            }
            ')' => {
                it.next();
                Token::RightParen
            }
            ';' => {
                it.next();
                if it.next_if_eq(&';').is_some() {
                    Token::CaseEnd(false)
                } else if it.next_if_eq(&'&').is_some() {
                    Token::CaseEnd(true)
                } else {
                    Token::End(Condition::Always)
                }
            }
            c if c.is_ascii_whitespace() => {
                it.next();
                continue;
            }
            '#' => {
                while it.next_if(|v| *v != '\n').is_some() {}
                continue;
            }
            '|' => {
                it.next();
                if it.next_if_eq(&'|').is_some() {
                    Token::End(Condition::Failure)
                } else {
                    Token::Pipe
                }
            }
            '&' => {
                it.next();
                if it.next_if_eq(&'&').is_some() {
                    Token::End(Condition::Success)
                } else {
                    Token::Background
                }
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
                    if op == '<' && digits.next_if_eq(&'<').is_some() {
                        let strip_tabs = digits.next_if_eq(&'-').is_some();
                        let document = HereDocument::delimiter(&mut digits, strip_tabs, depth)?;
                        *it = digits;
                        return Ok(Some(Token::Here(fd, Rc::new(document))));
                    }
                    let mode = if digits.next_if_eq(&'&').is_some() {
                        Mode::Duplicate
                    } else if op == '<' && digits.next_if_eq(&'>').is_some() {
                        Mode::ReadWrite
                    } else if op == '<' {
                        Mode::Read
                    } else if digits.next_if_eq(&'>').is_some() {
                        Mode::Append
                    } else {
                        digits.next_if_eq(&'|');
                        Mode::Write
                    };
                    *it = digits;
                    Token::Redirect(fd, mode)
                } else {
                    let (parts, quoted) = word_parts(it, depth, false, false)?;
                    Token::Word(parts, quoted)
                }
            }
        };
        return Ok(Some(token));
    }
    Ok(None)
}
