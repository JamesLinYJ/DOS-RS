//! Script input stays separate from command stdin. Dot owns its descriptor
//! and return boundary; eval executes in the caller's existing return scope.
#![forbid(unsafe_code)]
use super::{Shell, close, diagnostic, input::Lines, open, redirection, syntax, write_all};
use alloc::{ffi::CString, format, string::String};
use dosrs_user::stat::Stat;

struct Script(i32);

impl Script {
    fn open(path: &str) -> Result<Self, i32> {
        let path = CString::new(path).map_err(|_| 22)?;
        let fd = open(&path, 0)?;
        match Stat::fd(fd) {
            Ok(info) if info.mode & 0o170000 != 0o040000 => (),
            result => {
                let _ = close(fd);
                return Err(result.err().unwrap_or(21));
            }
        }
        redirection::script_descriptor(fd).map(Self)
    }

    fn find(name: &str, path: &str) -> Result<Self, i32> {
        if name.contains('/') {
            return Self::open(name);
        }
        let mut reason = 2;
        for dir in path.split(':') {
            let candidate = if dir.is_empty() {
                name.into()
            } else {
                format!("{dir}/{name}")
            };
            match Self::open(&candidate) {
                Ok(script) => return Ok(script),
                Err(2 | 20 | 36) => (),
                Err(e @ (13 | 21)) => reason = e,
                Err(e) => return Err(e),
            }
        }
        Err(reason)
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        let _ = close(self.0);
    }
}

impl Shell {
    fn source_error(&mut self, message: &str) -> i32 {
        diagnostic(message);
        self.exiting = !self.interactive;
        2
    }

    fn source_available(&mut self) -> bool {
        if self.function_depth + self.source_depth + self.eval_depth < 64 {
            true
        } else {
            self.source_error("script evaluation nesting limit");
            false
        }
    }

    pub(super) fn dot(&mut self, args: &[String]) -> i32 {
        let delimiter = args.first().is_some_and(|s| s == "--");
        let args = if delimiter { &args[1..] } else { args };
        let Some((name, positional)) = args.split_first() else {
            return self.source_error(".: expected a file");
        };
        if !delimiter && name.starts_with('-') && name != "-" {
            return self.source_error(&format!(".: unknown option {name}"));
        }
        if !self.source_available() {
            return 2;
        }
        let script = match Script::find(name, self.get("PATH")) {
            Ok(script) => script,
            Err(e) => return self.source_error(&format!(".: {name}: open error {e}")),
        };
        // Optional operands are a documented extension. Without them, set and
        // shift in the file operate on the caller's positional parameters.
        let previous = if positional.is_empty() {
            None
        } else {
            let mut values = alloc::vec![self.args.first().cloned().unwrap_or_default()];
            values.extend_from_slice(positional);
            Some(core::mem::replace(&mut self.args, values))
        };
        self.source_depth += 1;
        let status = self.run_input(script.0, true, false);
        self.source_depth -= 1;
        self.returning = false;
        if let Some(args) = previous {
            self.args = args;
        }
        status
    }

    pub(super) fn eval(&mut self, args: &[String]) -> i32 {
        if !self.source_available() {
            return 2;
        }
        self.eval_depth += 1;
        self.run(&args.join(" "));
        self.eval_depth -= 1;
        self.last
    }

    pub(super) fn run_input(&mut self, fd: i32, script: bool, prompt: bool) -> i32 {
        let mut pending = String::new();
        // Stdin must not read ahead: a command may consume the next input line.
        let mut source = Lines::new(fd, script);
        let mut commands = false;
        while !self.exiting && !self.returning && self.flow.is_none() {
            if prompt {
                let _ = write_all(2, if pending.is_empty() { b"$ " } else { b"> " });
            }
            match source.next() {
                Ok(Some(line)) => {
                    pending.push_str(&line);
                    match syntax::parse(&pending) {
                        Ok(list) => {
                            commands |= !list.is_empty();
                            self.run_list(&list);
                            pending.clear();
                        }
                        Err(syntax::Error::Incomplete(_)) => pending.push('\n'),
                        Err(e) => {
                            commands = true;
                            self.last = self.source_error(e.message());
                            pending.clear();
                            if !prompt {
                                return self.last;
                            }
                        }
                    }
                }
                Ok(None) => {
                    if !pending.is_empty() {
                        self.run(&pending);
                        return self.last;
                    }
                    break;
                }
                Err(e) => return self.source_error(&format!("read error {e}")),
            }
        }
        if commands { self.last } else { 0 }
    }
}
