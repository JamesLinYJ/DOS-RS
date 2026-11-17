//! Recursive parsing of POSIX lists, pipelines and grouped commands.
use super::{
    Command, Compound, Condition, Error, Part, Pipeline, Redirect, Token, Word, identifier,
};
use alloc::{collections::VecDeque, string::String, vec::Vec};

#[derive(Clone, Copy, PartialEq)]
enum End {
    List,
    Group,
    Substitution,
    Case,
}

// Reading only as far as the grammar needs lets the closing parenthesis
// return directly to the outer word, even inside a double-quoted string.
struct Tokens<'a, 's> {
    source: &'a mut super::Input<'s>,
    ready: VecDeque<Token>,
    depth: u16,
    error: Option<Error>,
}
impl Tokens<'_, '_> {
    fn get(&mut self, index: usize) -> Option<&Token> {
        while self.ready.len() <= index && self.error.is_none() {
            match super::token(self.source, self.depth) {
                Ok(Some(t)) => self.ready.push_back(t),
                Ok(None) => break,
                Err(e) => self.error = Some(e),
            }
        }
        self.ready.get(index)
    }
    fn front(&mut self) -> Option<&Token> {
        self.get(0)
    }
    fn is_empty(&mut self) -> bool {
        self.front().is_none()
    }
    fn pop_front(&mut self) -> Option<Token> {
        self.front()?;
        self.ready.pop_front()
    }
}

struct Parser<'a, 's>(Tokens<'a, 's>);

impl Parser<'_, '_> {
    fn keyword(&mut self) -> Option<&str> {
        match self.0.front()? {
            Token::Word(parts, false) => match parts.as_slice() {
                [Part::Literal(word)] => Some(word),
                _ => None,
            },
            _ => None,
        }
    }
    fn take(&mut self, name: &str) -> bool {
        if self.keyword() != Some(name) {
            return false;
        }
        self.0.pop_front();
        true
    }
    fn need(&mut self, name: &'static str) -> Result<(), Error> {
        if self.take(name) {
            Ok(())
        } else {
            Err(self.expected(name))
        }
    }
    fn expected(&mut self, text: &'static str) -> Error {
        if self.0.is_empty() {
            Error::Incomplete(text)
        } else {
            Error::Invalid(text)
        }
    }
    fn newlines(&mut self) {
        while matches!(self.0.front(), Some(Token::Newline)) {
            self.0.pop_front();
        }
    }
    fn word(&mut self) -> Option<Word> {
        if !matches!(self.0.front(), Some(Token::Word(..))) {
            return None;
        }
        match self.0.pop_front()? {
            Token::Word(word, _) => Some(word),
            _ => unreachable!(),
        }
    }
    fn list(&mut self, ends: &[&str], end: End, depth: u16) -> Result<Vec<Pipeline>, Error> {
        if depth > 64 {
            return Err(Error::Invalid("compound commands nested too deeply"));
        }
        let mut out = Vec::new();
        let mut when = Condition::Always;
        loop {
            self.newlines();
            let stop = self.0.is_empty()
                || self.keyword().is_some_and(|s| ends.contains(&s))
                || matches!(end, End::Group | End::Substitution)
                    && matches!(self.0.front(), Some(Token::RightParen))
                || end == End::Case && matches!(self.0.front(), Some(Token::CaseEnd(_)));
            if stop {
                if when != Condition::Always
                    || out.is_empty() && end != End::Case && (!ends.is_empty() || end == End::Group)
                {
                    return Err(self.expected("expected command"));
                }
                return Ok(out);
            }
            let negate = self.take("!");
            let mut commands = Vec::new();
            loop {
                commands.push(self.command(depth)?);
                if !matches!(self.0.front(), Some(Token::Pipe)) {
                    break;
                }
                self.0.pop_front();
                self.newlines();
            }
            let background = matches!(self.0.front(), Some(Token::Background));
            out.push(Pipeline {
                commands,
                when,
                background,
                negate,
            });
            when = match self.0.front() {
                None => Condition::Always,
                Some(Token::RightParen) if matches!(end, End::Group | End::Substitution) => {
                    Condition::Always
                }
                Some(Token::CaseEnd(_)) if end == End::Case => Condition::Always,
                Some(Token::Newline | Token::Background) => {
                    self.0.pop_front();
                    Condition::Always
                }
                Some(Token::End(condition)) => {
                    let next = *condition;
                    self.0.pop_front();
                    next
                }
                _ => return Err(Error::Invalid("expected command separator")),
            };
        }
    }
    fn command(&mut self, depth: u16) -> Result<Command, Error> {
        let mut cmd = Command::default();
        let nested = depth + 1;
        if nested > 64 {
            return Err(Error::Invalid("compound commands nested too deeply"));
        }
        let function = self
            .keyword()
            .filter(|name| identifier(name))
            .map(String::from)
            .filter(|_| {
                matches!(self.0.get(1), Some(Token::LeftParen))
                    && matches!(self.0.get(2), Some(Token::RightParen))
            });
        cmd.compound = if let Some(name) = function {
            self.0.pop_front();
            self.0.pop_front();
            if !matches!(self.0.pop_front(), Some(Token::RightParen)) {
                return Err(Error::Invalid("expected ) after function name"));
            }
            self.newlines();
            let body = self.command(nested)?;
            if body.compound.is_none() {
                return Err(Error::Invalid("expected function body"));
            }
            Some(Compound::Function {
                name,
                body: alloc::rc::Rc::new(body),
            })
        } else if matches!(self.0.front(), Some(Token::LeftParen)) {
            self.0.pop_front();
            let body = self.list(&[], End::Group, nested)?;
            if !matches!(self.0.front(), Some(Token::RightParen)) {
                return Err(self.expected("expected )"));
            }
            self.0.pop_front();
            Some(Compound::Subshell(body))
        } else if self.take("{") {
            let body = self.list(&["}"], End::List, nested)?;
            self.need("}")?;
            Some(Compound::Group(body))
        } else if self.take("case") {
            Some(self.case(nested)?)
        } else if self.take("if") {
            let mut arms = Vec::new();
            loop {
                let test = self.list(&["then"], End::List, nested)?;
                self.need("then")?;
                let body = self.list(&["elif", "else", "fi"], End::List, nested)?;
                arms.push((test, body));
                if !self.take("elif") {
                    break;
                }
            }
            let otherwise = if self.take("else") {
                self.list(&["fi"], End::List, nested)?
            } else {
                Vec::new()
            };
            self.need("fi")?;
            Some(Compound::If { arms, otherwise })
        } else if matches!(self.keyword(), Some("while" | "until")) {
            let until = self.take("until");
            if !until {
                self.need("while")?;
            }
            let test = self.list(&["do"], End::List, nested)?;
            let body = self.do_group(nested)?;
            Some(Compound::While { until, test, body })
        } else if self.take("for") {
            let Some(name) = self
                .keyword()
                .filter(|name| identifier(name))
                .map(String::from)
            else {
                return Err(self.expected("expected for variable"));
            };
            self.0.pop_front();
            self.newlines();
            let words = if self.take("in") {
                let mut words = Vec::new();
                while let Some(word) = self.word() {
                    words.push(word);
                }
                if !matches!(
                    self.0.front(),
                    Some(Token::Newline | Token::End(Condition::Always))
                ) {
                    return Err(self.expected("expected separator after for words"));
                }
                Some(words)
            } else {
                None
            };
            if matches!(self.0.front(), Some(Token::End(Condition::Always))) {
                self.0.pop_front();
            }
            self.newlines();
            let body = self.do_group(nested)?;
            Some(Compound::For { name, words, body })
        } else {
            if matches!(
                self.keyword(),
                Some("then" | "else" | "elif" | "fi" | "do" | "done" | "}" | "esac")
            ) {
                return Err(Error::Invalid("unexpected reserved word"));
            }
            None
        };
        loop {
            if let Some(Token::Redirect(..)) = self.0.front() {
                let Some(Token::Redirect(fd, mode)) = self.0.pop_front() else {
                    unreachable!()
                };
                let target = self
                    .word()
                    .ok_or_else(|| self.expected("expected redirection path"))?;
                cmd.redirects.push(Redirect { fd, mode, target });
            } else if cmd.compound.is_none()
                && let Some(word) = self.word()
            {
                cmd.words.push(word);
            } else {
                break;
            }
        }
        if cmd.compound.is_none() && cmd.words.is_empty() && cmd.redirects.is_empty() {
            Err(self.expected("expected command"))
        } else {
            Ok(cmd)
        }
    }
    fn case(&mut self, depth: u16) -> Result<Compound, Error> {
        let value = self
            .word()
            .ok_or_else(|| self.expected("expected case word"))?;
        self.newlines();
        self.need("in")?;
        let mut arms = Vec::new();
        loop {
            self.newlines();
            if self.take("esac") {
                break;
            }
            if matches!(self.0.front(), Some(Token::LeftParen)) {
                self.0.pop_front();
            }
            let mut patterns = Vec::new();
            loop {
                patterns.push(
                    self.word()
                        .ok_or_else(|| self.expected("expected case pattern"))?,
                );
                if !matches!(self.0.front(), Some(Token::Pipe)) {
                    break;
                }
                self.0.pop_front();
            }
            if !matches!(self.0.front(), Some(Token::RightParen)) {
                return Err(self.expected("expected ) after case pattern"));
            }
            self.0.pop_front();
            let body = self.list(&["esac"], End::Case, depth)?;
            let fall_through = if let Some(Token::CaseEnd(fall)) = self.0.front() {
                let fall = *fall;
                self.0.pop_front();
                fall
            } else {
                false
            };
            arms.push(super::CaseArm {
                patterns,
                body,
                fall_through,
            });
        }
        Ok(Compound::Case { value, arms })
    }
    fn do_group(&mut self, depth: u16) -> Result<Vec<Pipeline>, Error> {
        self.need("do")?;
        let body = self.list(&["done"], End::List, depth)?;
        self.need("done")?;
        Ok(body)
    }
}

fn parse_until(input: &mut super::Input<'_>, end: End, depth: u16) -> Result<Vec<Pipeline>, Error> {
    let mut parser = Parser(Tokens {
        source: input,
        ready: VecDeque::new(),
        depth,
        error: None,
    });
    let result = parser.list(&[], end, depth);
    if let Some(error) = parser.0.error.take() {
        return Err(error);
    }
    let commands = result?;
    if end == End::Substitution && !matches!(parser.0.pop_front(), Some(Token::RightParen)) {
        return Err(Error::Incomplete("unterminated command substitution"));
    }
    Ok(commands)
}

pub(super) fn substitution(
    input: &mut super::Input<'_>,
    depth: u16,
) -> Result<Vec<Pipeline>, Error> {
    parse_until(input, End::Substitution, depth)
}

pub(super) fn parse_input(
    input: &mut super::Input<'_>,
    depth: u16,
) -> Result<Vec<Pipeline>, Error> {
    parse_until(input, End::List, depth)
}

pub fn parse(line: &str) -> Result<Vec<Pipeline>, Error> {
    parse_input(&mut line.chars().peekable(), 0)
}
