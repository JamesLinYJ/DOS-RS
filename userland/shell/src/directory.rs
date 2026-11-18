//! Shell directory state and Issue 8 cd/pwd policy.
#![forbid(unsafe_code)]
use super::{Shell, Variable, diagnostic, write_all};
use alloc::{ffi::CString, format, string::String, vec::Vec};
use dosrs_user::{stat::Stat, working_directory};

pub(super) fn physical() -> Result<String, i32> {
    String::from_utf8(working_directory::current()?).map_err(|_| 84)
}

fn directory(path: &str) -> Result<Stat, i32> {
    let path = CString::new(path).map_err(|_| 22)?;
    let info = Stat::file(Some(&path))?;
    if info.mode & 0o170000 == 0o040000 {
        Ok(info)
    } else {
        Err(20)
    }
}

fn valid_logical(path: &str) -> bool {
    if !path.starts_with('/') || path.split('/').any(|p| matches!(p, "." | "..")) {
        return false;
    }
    match (directory(path), directory(".")) {
        (Ok(path), Ok(current)) => path.dev == current.dev && path.ino == current.ino,
        _ => false,
    }
}

fn join(parent: &str, child: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{child}")
    } else {
        format!("{parent}/{child}")
    }
}

fn canonical(path: &str) -> Result<String, i32> {
    let root = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else {
        "/"
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if !parts.is_empty() => {
                // Lexical removal must not turn missing/.. or file/.. into
                // a successful change. Following a directory symlink here
                // verifies its type but still removes its logical component.
                directory(&format!("{root}{}", parts.join("/")))?;
                parts.pop();
            }
            ".." => {}
            _ => parts.push(part),
        }
    }
    Ok(format!("{root}{}", parts.join("/")))
}

#[derive(Default)]
struct Options {
    physical: bool,
    require_physical: bool,
    first: usize,
}

fn options(args: &[String], change: bool) -> Result<Options, ()> {
    let mut result = Options::default();
    for arg in args {
        if arg == "--" {
            result.first += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        for flag in arg[1..].chars() {
            match flag {
                'L' => result.physical = false,
                'P' => result.physical = true,
                'e' if change => result.require_physical = true,
                _ => return Err(()),
            }
        }
        result.first += 1;
    }
    if args.len() - result.first > usize::from(change) {
        return Err(());
    }
    Ok(result)
}

fn failure(command: &str, path: &str, error: i32) -> i32 {
    dosrs_user::cli::diagnostic(format!("sh: {command}").as_bytes(), path.as_bytes(), error);
    1
}

fn print(command: &str, path: &str) -> i32 {
    match write_all(1, format!("{path}\n").as_bytes()) {
        Ok(()) => 0,
        Err(error) => failure(command, "output", error),
    }
}

impl Shell {
    pub(super) fn initialize_directory(&mut self) {
        let value = self.logical_directory().or_else(|| physical().ok());
        if let Some(value) = value {
            self.vars.insert(
                "PWD".into(),
                Variable {
                    value: Some(value),
                    exported: true,
                    readonly: false,
                },
            );
        } else {
            // An inaccessible or unnamed cwd has no reliable absolute name.
            self.vars.remove("PWD");
        }
    }

    fn logical_directory(&self) -> Option<String> {
        let path = self.get("PWD");
        valid_logical(path).then(|| path.into())
    }

    pub(super) fn pwd(&self, args: &[String]) -> i32 {
        let Ok(options) = options(args, false) else {
            diagnostic("pwd: invalid option or unexpected operand");
            return 2;
        };
        let path = if options.physical {
            physical()
        } else {
            self.logical_directory().map_or_else(physical, Ok)
        };
        match path {
            Ok(path) => print("pwd", &path),
            Err(error) => failure("pwd", "current directory", error),
        }
    }

    pub(super) fn cd(&mut self, args: &[String]) -> i32 {
        let Ok(options) = options(args, true) else {
            diagnostic("cd: invalid option or too many operands");
            return 2;
        };
        let operand = args
            .get(options.first)
            .map_or(self.get("HOME"), String::as_str);
        let previous = operand == "-";
        let destination = if previous {
            self.get("OLDPWD")
        } else {
            operand
        };
        if destination.is_empty() || previous && !destination.starts_with('/') {
            diagnostic(if previous {
                "cd: OLDPWD is not an absolute pathname"
            } else if args.get(options.first).is_none() {
                "cd: HOME is not set"
            } else {
                "cd: empty directory operand"
            });
            return 1;
        }
        for name in ["PWD", "OLDPWD"] {
            if self.vars.get(name).is_some_and(|v| v.readonly) {
                diagnostic(&format!("cd: {name}: readonly variable"));
                return 1;
            }
        }
        let base = self.logical_directory().or_else(|| physical().ok());
        let old = base.clone().unwrap_or_else(|| self.get("PWD").into());
        let mut selected = String::from(destination);
        let mut display = previous;
        if !destination.starts_with('/')
            && !matches!(destination.split('/').next(), Some("." | ".."))
            && let Some(search) = self.vars.get("CDPATH").and_then(|v| v.value.as_deref())
        {
            for prefix in search.split(':') {
                let candidate = join(if prefix.is_empty() { "." } else { prefix }, destination);
                if directory(&candidate).is_ok() {
                    selected = candidate;
                    display |= !prefix.is_empty();
                    break;
                }
            }
        }
        let logical = if !options.physical && (selected.starts_with('/') || base.is_some()) {
            let absolute = if selected.starts_with('/') {
                selected.clone()
            } else {
                join(base.as_deref().unwrap(), &selected)
            };
            match canonical(&absolute) {
                Ok(path) => Some(path),
                Err(error) => return failure("cd", destination, error),
            }
        } else {
            None
        };
        let path = logical.as_deref().unwrap_or(&selected);
        // POSIX permits conversion to an equivalent relative path even below
        // PATH_MAX. This also permits a short operand in a very deep cwd.
        let path = if let Some(base) = base.as_deref().filter(|_| logical.is_some()) {
            if path == base {
                "."
            } else {
                path.strip_prefix(&join(base, "")).unwrap_or(path)
            }
        } else {
            path
        };
        let path = match CString::new(path) {
            Ok(path) => path,
            Err(_) => return failure("cd", destination, 22),
        };
        if let Err(error) = working_directory::change(&path) {
            return failure("cd", destination, error);
        }
        let new = match logical.map_or_else(physical, Ok) {
            Ok(path) => path,
            Err(error) if options.physical && options.require_physical => {
                return failure("cd", "new working directory", error);
            }
            Err(_) => return 0,
        };
        // Readonly attributes were checked before changing the process cwd.
        if self.set("OLDPWD", &old).is_err() || self.set("PWD", &new).is_err() {
            return 1;
        }
        self.vars.get_mut("PWD").unwrap().exported = true;
        if display { print("cd", &new) } else { 0 }
    }
}
