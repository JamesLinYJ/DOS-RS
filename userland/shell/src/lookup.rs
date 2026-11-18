//! Resolve command prefixes before deciding whether execution needs a child.
//! Querying a name never forks or changes the shell's environment.
#![forbid(unsafe_code)]
use super::{CommandKind, Shell, cwd, diagnostic, write_all};
use alloc::{ffi::CString, format, string::String};
use dosrs_user::{effective_access, stat::Stat};

pub(super) const DEFAULT_PATH: &str = "/bin";

#[derive(Clone, Copy)]
pub(super) enum Action {
    Execute {
        first: usize,
        kind: CommandKind,
        wrapped: bool,
        default_path: bool,
    },
    Describe {
        first: usize,
        verbose: bool,
        default_path: bool,
    },
    Empty,
    Invalid,
}

impl Action {
    pub(super) fn external(self) -> bool {
        matches!(
            self,
            Self::Execute {
                kind: CommandKind::External,
                ..
            }
        )
    }

    pub(super) fn special(self) -> bool {
        matches!(
            self,
            Self::Execute {
                kind: CommandKind::Special,
                wrapped: false,
                ..
            }
        )
    }

    pub(super) fn permanent_redirect(self, words: &[String]) -> bool {
        matches!(self, Self::Execute { first, kind: CommandKind::Special, .. }
            if words[first..] == ["exec"])
    }
}

impl Shell {
    pub(super) fn resolve(&self, words: &[String]) -> Action {
        let mut first = 0;
        let mut wrapped = false;
        let mut default_path = false;
        while let Some(name) = words.get(first) {
            let kind = self.command_kind(name, !wrapped);
            if name != "command" || kind != CommandKind::Builtin {
                return Action::Execute {
                    first,
                    kind,
                    wrapped,
                    default_path,
                };
            }
            wrapped = true;
            first += 1;
            let mut describe = None;
            while let Some(option) = words.get(first) {
                if option == "--" {
                    first += 1;
                    break;
                }
                if !option.starts_with('-') || option == "-" {
                    break;
                }
                for flag in option[1..].chars() {
                    match flag {
                        'p' => default_path = true,
                        'v' | 'V' => {
                            let verbose = flag == 'V';
                            if describe.is_some_and(|old| old != verbose) {
                                return Action::Invalid;
                            }
                            describe = Some(verbose);
                        }
                        _ => return Action::Invalid,
                    }
                }
                first += 1;
            }
            if let Some(verbose) = describe {
                return if first < words.len() {
                    Action::Describe {
                        first,
                        verbose,
                        default_path,
                    }
                } else {
                    Action::Invalid
                };
            }
        }
        Action::Empty
    }

    pub(super) fn declaration(&self, words: &[String]) -> bool {
        matches!(self.resolve(words), Action::Execute { first, .. }
            if matches!(words[first].as_str(), "export" | "readonly"))
    }

    pub(super) fn special_failure(&mut self) {
        if !self.regular_builtin {
            self.exiting = !self.interactive;
        }
    }

    pub(super) fn describe(&self, names: &[String], verbose: bool, default_path: bool) -> i32 {
        let path = if default_path {
            DEFAULT_PATH
        } else {
            self.get("PATH")
        };
        let mut status = 0;
        for name in names {
            let description = if matches!(
                name.as_str(),
                "!" | "{"
                    | "}"
                    | "case"
                    | "do"
                    | "done"
                    | "elif"
                    | "else"
                    | "esac"
                    | "fi"
                    | "for"
                    | "if"
                    | "in"
                    | "then"
                    | "until"
                    | "while"
            ) {
                Some((name.clone(), "reserved word"))
            } else {
                match self.command_kind(name, true) {
                    CommandKind::Special => Some((name.clone(), "special shell built-in")),
                    CommandKind::Builtin => Some((name.clone(), "shell built-in")),
                    CommandKind::Function => Some((name.clone(), "shell function")),
                    CommandKind::External => executable(name, path).map(|p| (p, "executable")),
                }
            };
            if let Some((value, category)) = description {
                let output = if verbose {
                    format!("{name}: {category}: {value}\n")
                } else {
                    format!("{value}\n")
                };
                if write_all(1, output.as_bytes()).is_err() {
                    diagnostic("command: output error");
                    return 1;
                }
            } else {
                if verbose {
                    diagnostic(&format!("command: {name}: not found"));
                }
                status = 1;
            }
        }
        status
    }
}

fn executable(name: &str, path: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let candidates = path.split(':').map(|dir| {
        if dir.is_empty() {
            name.into()
        } else {
            format!("{dir}/{name}")
        }
    });
    if name.contains('/') {
        executable_path(name)
    } else {
        candidates.filter_map(|p| executable_path(&p)).next()
    }
}

fn executable_path(path: &str) -> Option<String> {
    // The query tests effective IDs (as exec does), not access()'s real IDs.
    // A searchable directory is not an executable utility.
    let absolute = if path.starts_with('/') {
        path.into()
    } else {
        format!("{}/{path}", cwd()?)
    };
    let cpath = CString::new(absolute.as_str()).ok()?;
    let info = Stat::file(Some(&cpath)).ok()?;
    if info.mode & 0o170000 != 0o100000 || effective_access(&cpath, 1).is_err() {
        return None;
    }
    Some(absolute)
}
