//! Option and positional-parameter handling shared by sh and the set built-in.
use super::{Shell, diagnostic, format, write_all};
use alloc::string::String;

#[derive(Default)]
pub(super) struct Invocation {
    pub command: bool,
    pub stdin: bool,
    pub interactive: bool,
    pub first: usize,
    positional: bool,
}

impl Shell {
    pub(super) fn option_flags(&self) -> String {
        let mut flags = String::new();
        for (ch, active) in [
            ('f', self.noglob),
            ('i', self.interactive),
            ('m', self.monitor),
            ('u', self.nounset),
        ] {
            if active {
                flags.push(ch);
            }
        }
        flags
    }

    pub(super) fn options(&mut self, args: &[String], startup: bool) -> Result<Invocation, ()> {
        let mut parsed = Invocation::default();
        while let Some(arg) = args.get(parsed.first) {
            if arg == "--" {
                parsed.first += 1;
                parsed.positional = true;
                break;
            }
            if arg.len() < 2 || !(arg.starts_with('-') || arg.starts_with('+')) {
                break;
            }
            let enabled = arg.starts_with('-');
            parsed.first += 1;
            for flag in arg[1..].chars() {
                match flag {
                    'f' => self.noglob = enabled,
                    'u' => self.nounset = enabled,
                    'c' if startup && enabled => parsed.command = true,
                    's' if startup && enabled => parsed.stdin = true,
                    'i' if startup && enabled => parsed.interactive = true,
                    'o' => {
                        if let Some(name) = args.get(parsed.first) {
                            match name.as_str() {
                                "noglob" => self.noglob = enabled,
                                "nounset" => self.nounset = enabled,
                                _ => {
                                    diagnostic(&format!("unknown option {name}"));
                                    return Err(());
                                }
                            }
                            parsed.first += 1;
                        } else if !startup {
                            for (name, on) in [("noglob", self.noglob), ("nounset", self.nounset)] {
                                let line = if enabled {
                                    format!("{name} {}\n", if on { "on" } else { "off" })
                                } else {
                                    format!("set {}o {name}\n", if on { '-' } else { '+' })
                                };
                                write_all(1, line.as_bytes())
                                    .map_err(|_| diagnostic("set: output error"))?;
                            }
                        } else {
                            diagnostic("missing option name");
                            return Err(());
                        }
                    }
                    _ => {
                        diagnostic(&format!("unknown option {flag}"));
                        return Err(());
                    }
                }
            }
        }
        parsed.positional |= parsed.first < args.len();
        Ok(parsed)
    }

    pub(super) fn set_arguments(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            for (key, var) in &self.vars {
                let Some(value) = &var.value else {
                    continue;
                };
                let value = value.replace('\'', "'\\''");
                if write_all(1, format!("{key}='{value}'\n").as_bytes()).is_err() {
                    return 1;
                }
            }
            return 0;
        }
        let Ok(opts) = self.options(args, false) else {
            return 2;
        };
        if opts.positional {
            self.args.truncate(1);
            self.args.extend_from_slice(&args[opts.first..]);
        }
        0
    }

    pub(super) fn shift(&mut self, args: &[String]) -> i32 {
        let n = match args {
            [] => Some(1),
            [n] => n.parse::<usize>().ok(),
            _ => None,
        };
        if let Some(n) = n.filter(|&n| n < self.args.len()) {
            self.args.drain(1..1 + n);
            0
        } else {
            diagnostic("shift: expected a count within the positional parameters");
            1
        }
    }
}
