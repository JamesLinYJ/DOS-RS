//! Parse native pipelines before opening any output files.
use alloc::{string::String, vec::Vec};
use dosrs_native_posix::Program;

enum Token {
    Word(String),
    Pipe,
    Input,
    Output,
}

fn tokenize(line: &str) -> Result<Vec<Token>, &'static str> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' => {
                chars.next();
            }
            '|' | '<' | '>' => {
                chars.next();
                tokens.push(match ch {
                    '|' => Token::Pipe,
                    '<' => Token::Input,
                    _ => Token::Output,
                });
            }
            _ => {
                let mut word = String::new();
                while let Some(&ch) = chars.peek() {
                    if matches!(ch, ' ' | '\t' | '|' | '<' | '>') {
                        break;
                    }
                    chars.next();
                    if matches!(ch, '\'' | '"') {
                        loop {
                            let quoted = chars.next().ok_or("unterminated quote")?;
                            if quoted == ch {
                                break;
                            }
                            word.push(quoted);
                        }
                    } else {
                        word.push(ch);
                    }
                }
                tokens.push(Token::Word(word));
            }
        }
    }
    Ok(tokens)
}

pub(super) struct Stage {
    pub program: Program,
    pub argv: Vec<String>,
    pub input: Option<String>,
    pub output: Option<String>,
}

pub(super) fn parse(line: &str) -> Result<Vec<Stage>, &'static str> {
    let mut stages = Vec::new();
    let mut tokens = tokenize(line)?.into_iter().peekable();
    loop {
        let mut argv: Vec<String> = Vec::new();
        let mut input = None;
        let mut output = None;
        while let Some(token) = tokens.next() {
            match token {
                Token::Pipe => {
                    if tokens.peek().is_none() {
                        return Err("missing pipeline command");
                    }
                    break;
                }
                Token::Word(s) => argv.push(s),
                Token::Input | Token::Output => {
                    let dest = if matches!(token, Token::Input) {
                        &mut input
                    } else {
                        &mut output
                    };
                    let Some(Token::Word(path)) = tokens.next() else {
                        return Err("missing redirection path");
                    };
                    if dest.replace(path).is_some() {
                        return Err("duplicate redirection");
                    }
                }
            }
        }
        let name = argv.first().ok_or("missing command")?;
        let program = Program::lookup(name).ok_or("unknown native program")?;
        // Four terminals share the native heap. Bound the number of simultaneous
        // continuations even for a long command line read from a batch file.
        if stages.len() == 32 {
            return Err("too many pipeline stages");
        }
        stages.push(Stage {
            program,
            argv,
            input,
            output,
        });
        if tokens.peek().is_none() {
            return Ok(stages);
        }
    }
}
