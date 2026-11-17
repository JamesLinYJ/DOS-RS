//! Compound commands use the existing shell environment unless a pipeline
//! or a parenthesized group has already forked a child.
use super::{Shell, diagnostic, syntax::Compound};
use alloc::{string::String, vec::Vec};

#[derive(Clone, Copy)]
pub(super) struct LoopControl {
    leave: bool,
    levels: usize,
}

unsafe extern "C" {
    fn fnmatch(pattern: *const i8, subject: *const i8, flags: i32) -> i32;
}

impl Shell {
    pub(super) fn call_function(
        &mut self,
        body: &super::syntax::Command,
        words: &[String],
        assignments: Vec<(String, String)>,
    ) -> i32 {
        if self.function_depth >= 64 {
            diagnostic("function call nesting limit");
            return 2;
        }
        let mut positionals = alloc::vec![self.args.first().cloned().unwrap_or_default()];
        positionals.extend_from_slice(&words[1..]);
        let previous = core::mem::replace(&mut self.args, positionals);
        let mut saved = Vec::new();
        for (name, value) in assignments {
            let old = self.vars.insert(
                name.clone(),
                super::Variable {
                    value,
                    exported: true,
                },
            );
            saved.push((name, old));
        }
        self.function_depth += 1;
        let status = if matches!(body.compound, Some(Compound::Subshell(_))) {
            let pid = unsafe { super::fork() };
            match pid {
                0 => {
                    self.monitor = false;
                    super::exit(self.command(body, true));
                }
                -1 => 1,
                child => super::wait(child).unwrap_or(127),
            }
        } else {
            self.command(body, false)
        };
        self.function_depth -= 1;
        self.returning = false;
        self.args = previous;
        for (name, old) in saved.into_iter().rev() {
            if let Some(var) = old {
                self.vars.insert(name, var);
            } else {
                self.vars.remove(&name);
            }
        }
        status
    }

    pub(super) fn loop_control(&mut self, leave: bool, args: &[String]) -> i32 {
        let levels = match args {
            [] => 1,
            [n] => match n.parse::<usize>() {
                Ok(n) if n != 0 => n,
                _ => {
                    diagnostic("expected positive loop count");
                    return 2;
                }
            },
            _ => {
                diagnostic("expected at most one loop count");
                return 2;
            }
        };
        if self.loop_depth == 0 {
            return 0;
        }
        self.flow = Some(LoopControl {
            leave,
            levels: levels.min(self.loop_depth),
        });
        0
    }

    fn leave_loop(&mut self) -> bool {
        if self.exiting || self.returning {
            return true;
        }
        let Some(mut flow) = self.flow.take() else {
            return false;
        };
        if flow.levels > 1 {
            flow.levels -= 1;
            self.flow = Some(flow);
            true
        } else {
            flow.leave
        }
    }

    fn matches_pattern(
        &mut self,
        word: &super::Word,
        subject: &core::ffi::CStr,
    ) -> Result<bool, ()> {
        use super::Part;
        let mut pattern = String::new();
        for part in word {
            let (text, quoted) = match part {
                Part::Literal(text) => (text.clone(), false),
                Part::Quoted(text) => (text.clone(), true),
                Part::Parameter(key, quoted) => (self.parameter(key), *quoted),
                Part::Substitution(body, quoted) => (self.capture(body)?, *quoted),
                Part::Arithmetic(word, quoted) => (self.arithmetic(word)?, *quoted),
            };
            for ch in text.chars() {
                if quoted {
                    pattern.push('\\');
                }
                pattern.push(ch);
            }
        }
        Ok(alloc::ffi::CString::new(pattern)
            .is_ok_and(|p| unsafe { fnmatch(p.as_ptr(), subject.as_ptr(), 0) == 0 }))
    }

    pub(super) fn compound(&mut self, command: &Compound) -> i32 {
        match command {
            Compound::Case { value, arms } => {
                let Ok(value) = self.expand(value, false) else {
                    return 1;
                };
                let Ok(subject) = alloc::ffi::CString::new(value.concat()) else {
                    return 1;
                };
                let mut fall = false;
                let mut status = 0;
                for arm in arms {
                    if !fall {
                        let mut matched = false;
                        for pattern in &arm.patterns {
                            match self.matches_pattern(pattern, &subject) {
                                Ok(true) => {
                                    matched = true;
                                    break;
                                }
                                Ok(false) => (),
                                Err(()) => return 1,
                            }
                        }
                        if !matched {
                            continue;
                        }
                    }
                    status = if arm.body.is_empty() {
                        0
                    } else {
                        self.run_list(&arm.body)
                    };
                    fall = arm.fall_through;
                    if !fall || self.exiting || self.returning || self.flow.is_some() {
                        break;
                    }
                }
                status
            }
            Compound::Function { name, body } => {
                self.functions.insert(name.clone(), body.clone());
                0
            }
            Compound::Group(body) | Compound::Subshell(body) => self.run_list(body),
            Compound::If { arms, otherwise } => {
                for (test, body) in arms {
                    let status = self.run_list(test);
                    if self.exiting || self.returning || self.flow.is_some() {
                        return status;
                    }
                    if status == 0 {
                        return self.run_list(body);
                    }
                }
                if otherwise.is_empty() {
                    0
                } else {
                    self.run_list(otherwise)
                }
            }
            Compound::While { until, test, body } => {
                self.loop_depth += 1;
                let mut status = 0;
                loop {
                    let condition = self.run_list(test);
                    if self.exiting || self.returning || self.flow.is_some() {
                        status = condition;
                        if self.leave_loop() {
                            break;
                        }
                        continue;
                    }
                    if (condition == 0) == *until {
                        break;
                    }
                    status = self.run_list(body);
                    if self.leave_loop() {
                        break;
                    }
                }
                self.loop_depth -= 1;
                status
            }
            Compound::For { name, words, body } => {
                let values: Vec<String> = match words {
                    Some(words) => {
                        let mut fields = Vec::new();
                        for word in words {
                            let Ok(values) = self.expand(word, true) else {
                                return 1;
                            };
                            fields.extend(values);
                        }
                        fields
                    }
                    None => self.args.get(1..).unwrap_or_default().to_vec(),
                };
                self.loop_depth += 1;
                let mut status = 0;
                for value in values {
                    self.set(name, &value);
                    status = self.run_list(body);
                    if self.leave_loop() {
                        break;
                    }
                }
                self.loop_depth -= 1;
                status
            }
        }
    }
}
