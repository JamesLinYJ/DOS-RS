//! Integer arithmetic for shell expansions. Parse unevaluated branches as well,
//! without reading their variables or executing assignments in them.
use super::Shell;
use alloc::string::ToString;

type Result<T> = core::result::Result<T, &'static str>;

#[derive(Clone, Copy)]
enum Operand<'a> {
    Number(i64),
    Variable(&'a str),
}

impl Operand<'_> {
    fn value(self, shell: &Shell, active: bool) -> Result<i64> {
        if !active {
            return Ok(0);
        }
        match self {
            Self::Number(n) => Ok(n),
            Self::Variable(name) => match shell.get(name).trim() {
                "" => Ok(0),
                value => integer(value),
            },
        }
    }
}

fn integer(text: &str) -> Result<i64> {
    let neg = text.starts_with('-');
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (radix, digits) = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, hex)
    } else if digits.starts_with('0') {
        (8, digits)
    } else {
        (10, digits)
    };
    if digits.is_empty() || !digits.as_bytes()[0].is_ascii_hexdigit() {
        return Err("invalid integer");
    }
    let n = u64::from_str_radix(digits, radix).map_err(|_| "invalid integer")? as i64;
    Ok(if neg { n.wrapping_neg() } else { n })
}

// Longest tokens precede their prefixes. Assignments associate to the right.
const OPERATORS: &[(&str, u8)] = &[
    ("<<=", 2),
    (">>=", 2),
    ("+=", 2),
    ("-=", 2),
    ("*=", 2),
    ("/=", 2),
    ("%=", 2),
    ("&=", 2),
    ("^=", 2),
    ("|=", 2),
    ("||", 4),
    ("&&", 5),
    ("==", 9),
    ("!=", 9),
    ("<=", 10),
    (">=", 10),
    ("<<", 11),
    (">>", 11),
    (",", 1),
    ("=", 2),
    ("?", 3),
    ("|", 6),
    ("^", 7),
    ("&", 8),
    ("<", 10),
    (">", 10),
    ("+", 12),
    ("-", 12),
    ("*", 13),
    ("/", 13),
    ("%", 13),
];

struct Parser<'a> {
    rest: &'a str,
}

impl<'a> Parser<'a> {
    fn space(&mut self) {
        self.rest = self
            .rest
            .trim_start_matches(|c: char| c.is_ascii_whitespace());
    }

    fn take(&mut self, token: &str) -> bool {
        self.space();
        if let Some(rest) = self.rest.strip_prefix(token) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn operand(&mut self, shell: &mut Shell, active: bool, depth: u8) -> Result<Operand<'a>> {
        self.space();
        if self.rest.starts_with("++") || self.rest.starts_with("--") {
            return Err("unsupported increment or decrement");
        }
        if self.take("(") {
            let result = self.expr(shell, 1, active, depth + 1)?;
            if !self.take(")") {
                return Err("missing closing parenthesis");
            }
            return Ok(result);
        }
        for op in ["+", "-", "~", "!"] {
            if self.take(op) {
                let n = self
                    .expr(shell, 14, active, depth + 1)?
                    .value(shell, active)?;
                return Ok(Operand::Number(match op {
                    "-" => n.wrapping_neg(),
                    "~" => !n,
                    "!" => i64::from(n == 0),
                    _ => n,
                }));
            }
        }
        let end = self
            .rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if end == 0 {
            return Err("expected an integer or variable");
        }
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        if word.as_bytes()[0].is_ascii_digit() {
            integer(word).map(Operand::Number)
        } else {
            Ok(Operand::Variable(word))
        }
    }

    fn expr(
        &mut self,
        shell: &mut Shell,
        minimum: u8,
        active: bool,
        depth: u8,
    ) -> Result<Operand<'a>> {
        if depth >= 64 {
            return Err("arithmetic expression nested too deeply");
        }
        let mut left = self.operand(shell, active, depth)?;
        loop {
            self.space();
            if self.rest.starts_with("++") || self.rest.starts_with("--") {
                return Err("unsupported increment or decrement");
            }
            let Some(&(op, rank)) = OPERATORS.iter().find(|(op, _)| self.rest.starts_with(op))
            else {
                break;
            };
            if rank < minimum {
                break;
            }
            self.rest = &self.rest[op.len()..];
            let a = if op == "=" {
                0
            } else {
                left.value(shell, active)?
            };
            if op == "?" {
                let yes = self
                    .expr(shell, 1, active && a != 0, depth + 1)?
                    .value(shell, active && a != 0)?;
                if !self.take(":") {
                    return Err("missing colon in conditional");
                }
                let no = self
                    .expr(shell, rank, active && a == 0, depth + 1)?
                    .value(shell, active && a == 0)?;
                left = Operand::Number(if a != 0 { yes } else { no });
                continue;
            }
            let evaluate_right = active && !((op == "&&" && a == 0) || (op == "||" && a != 0));
            let rhs = self.expr(
                shell,
                if rank == 2 { rank } else { rank + 1 },
                evaluate_right,
                depth + 1,
            )?;
            let b = rhs.value(shell, evaluate_right)?;
            if rank == 2 {
                let Operand::Variable(name) = left else {
                    return Err("assignment requires a variable");
                };
                let n = if !active {
                    0
                } else if op == "=" {
                    b
                } else {
                    binary(&op[..op.len() - 1], a, b)?
                };
                if active {
                    shell
                        .set(name, &n.to_string())
                        .map_err(|_| "readonly variable")?;
                }
                left = Operand::Number(n);
            } else {
                left = Operand::Number(if active { binary(op, a, b)? } else { 0 });
            }
        }
        Ok(left)
    }
}

fn binary(op: &str, a: i64, b: i64) -> Result<i64> {
    Ok(match op {
        "," => b,
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" | "%" if b == 0 => return Err("division by zero"),
        "/" => a.wrapping_div(b),
        "%" => a.wrapping_rem(b),
        "<<" => a.wrapping_shl(b as u32),
        ">>" => a.wrapping_shr(b as u32),
        "^" => a ^ b,
        "&" => a & b,
        "|" => a | b,
        "<" => i64::from(a < b),
        "<=" => i64::from(a <= b),
        ">" => i64::from(a > b),
        ">=" => i64::from(a >= b),
        "==" => i64::from(a == b),
        "!=" => i64::from(a != b),
        "&&" => i64::from(a != 0 && b != 0),
        "||" => i64::from(a != 0 || b != 0),
        _ => return Err("unsupported arithmetic operator"),
    })
}

pub(super) fn evaluate(shell: &mut Shell, expression: &str) -> Result<i64> {
    let mut p = Parser { rest: expression };
    let value = p.expr(shell, 1, true, 0)?;
    p.space();
    if !p.rest.is_empty() {
        return Err("unexpected arithmetic operand");
    }
    value.value(shell, true)
}
