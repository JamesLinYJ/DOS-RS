//! Variable values and attributes belong to the shell execution environment.
//! An exported or readonly name can exist without having an assigned value.
use super::{Shell, diagnostic, syntax, write_all};
use alloc::{format, string::String};

impl Shell {
    pub(super) fn variable_value(&self, name: &str) -> Option<&str> {
        self.vars.get(name).and_then(|v| v.value.as_deref())
    }

    pub(super) fn check_variable(&self, name: &str) -> Result<(), ()> {
        if self.vars.get(name).is_some_and(|v| v.readonly) {
            diagnostic(&format!("{name}: readonly variable"));
            Err(())
        } else {
            Ok(())
        }
    }

    pub(super) fn set(&mut self, name: &str, value: &str) -> Result<(), ()> {
        self.check_variable(name)?;
        self.vars.entry(name.into()).or_default().value = Some(value.into());
        Ok(())
    }

    pub(super) fn unset_variable(&mut self, name: &str) -> Result<(), ()> {
        self.check_variable(name)?;
        self.vars.remove(name);
        Ok(())
    }

    fn variable_builtin_error(&mut self, message: &str) -> i32 {
        diagnostic(message);
        self.special_failure();
        1
    }

    pub(super) fn declare_variables(&mut self, utility: &str, args: &[String]) -> i32 {
        let mut args = args;
        let mut print = false;
        while let Some(option) = args.first().filter(|s| s.starts_with('-')) {
            args = &args[1..];
            match option.as_str() {
                "--" => break,
                "-p" => print = true,
                _ => return self.variable_builtin_error(&format!("{utility}: invalid option")),
            }
        }
        if print && !args.is_empty() {
            return self.variable_builtin_error(&format!("{utility}: -p takes no operands"));
        }
        let readonly = utility == "readonly";
        if args.is_empty() {
            for (name, variable) in &self.vars {
                if !(if readonly {
                    variable.readonly
                } else {
                    variable.exported
                }) {
                    continue;
                }
                let assignment = match &variable.value {
                    Some(value) => format!("{name}='{}'", value.replace('\'', "'\\''")),
                    None => name.clone(),
                };
                if write_all(1, format!("{utility} {assignment}\n").as_bytes()).is_err() {
                    return self.variable_builtin_error(&format!("{utility}: write error"));
                }
            }
            return 0;
        }
        for argument in args {
            let (name, value) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(name, value)| {
                    (name, Some(value))
                });
            if !syntax::identifier(name) {
                return self
                    .variable_builtin_error(&format!("{utility}: {name}: invalid variable name"));
            }
            if let Some(value) = value {
                if self.set(name, value).is_err() {
                    self.special_failure();
                    return 1;
                }
            }
            let variable = self.vars.entry(name.into()).or_default();
            if readonly {
                variable.readonly = true;
            } else {
                variable.exported = true;
            }
        }
        0
    }

    pub(super) fn unset(&mut self, args: &[String]) -> i32 {
        let mut args = args;
        let mut functions = None;
        while let Some(option) = args.first().filter(|s| s.starts_with('-')) {
            args = &args[1..];
            if option == "--" {
                break;
            }
            if option == "-" {
                return self.variable_builtin_error("unset: invalid option");
            }
            for flag in option[1..].chars() {
                let kind = match flag {
                    'f' => true,
                    'v' => false,
                    _ => return self.variable_builtin_error("unset: invalid option"),
                };
                if functions.is_some_and(|previous| previous != kind) {
                    return self.variable_builtin_error("unset: -f and -v are mutually exclusive");
                }
                functions = Some(kind);
            }
        }
        for name in args {
            if !syntax::identifier(name) {
                return self.variable_builtin_error(&format!("unset: {name}: invalid name"));
            }
            if functions == Some(true) {
                self.functions.remove(name);
            } else if self.unset_variable(name).is_err() {
                self.special_failure();
                return 1;
            }
        }
        0
    }
}
