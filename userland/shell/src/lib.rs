#![no_std]
extern crate alloc;
mod arithmetic;
mod compound;
mod directory;
mod expansion;
mod getopts;
mod here_input;
mod input;
mod jobs;
mod lookup;
mod options;
mod redirection;
mod source;
mod substitution;
mod syntax;
mod variables;

use alloc::{
    collections::BTreeMap,
    ffi::CString,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::ffi::CStr;
use dosrs_user::{Args, close, exit, open, read, write_all};
use syntax::{Condition, Part, Pipeline, Word};

// POSIX process functions are exported by the native userspace ABI.
unsafe extern "C" {
    fn fork() -> i32;
    fn execvpe(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32;
    fn waitpid(pid: i32, status: *mut i32, flags: i32) -> i32;
    fn getpid() -> i32;
    fn dup2(old: i32, new: i32) -> i32;
    fn pipe(fds: *mut i32) -> i32;
}
fn diagnostic(message: &str) {
    let _ = write_all(2, format!("sh: {message}\n").as_bytes());
}
fn error() -> i32 {
    dosrs_user::errno()
}
fn wait(pid: i32) -> Result<i32, i32> {
    let mut status = 0;
    loop {
        if unsafe { waitpid(pid, &mut status, 0) } >= 0 {
            return Ok(if status & 127 == 0 {
                status >> 8
            } else {
                128 + (status & 127)
            });
        }
        let e = error();
        if e != 4 {
            return Err(e);
        }
    }
}

#[derive(Clone, Default)]
struct Variable {
    value: Option<String>,
    exported: bool,
    readonly: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum CommandKind {
    Special,
    Function,
    Builtin,
    External,
}

struct Invocation {
    words: Vec<String>,
    assignments: Vec<(String, String)>,
    action: lookup::Action,
}

struct Shell {
    shell_pid: i32,
    substitution_status: Option<i32>,
    vars: BTreeMap<String, Variable>,
    functions: BTreeMap<String, alloc::rc::Rc<syntax::Command>>,
    function_depth: usize,
    source_depth: usize,
    eval_depth: usize,
    returning: bool,
    args: Vec<String>,
    last: i32,
    background: Vec<i32>,
    jobs: Vec<jobs::Job>,
    loop_depth: usize,
    flow: Option<compound::LoopControl>,
    monitor: bool,
    interactive: bool,
    noglob: bool,
    nounset: bool,
    exiting: bool,
    requested_exit: bool,
    regular_builtin: bool,
}
impl Shell {
    fn new(args: Vec<String>, env: &[String]) -> Self {
        let mut vars = BTreeMap::new();
        for s in env {
            if let Some((name, val)) = s.split_once('=') {
                vars.insert(
                    name.into(),
                    Variable {
                        value: Some(val.into()),
                        exported: true,
                        readonly: false,
                    },
                );
            }
        }
        vars.entry("IFS".into()).or_default().value = Some(" \t\n".into());
        vars.insert(
            "OPTIND".into(),
            Variable {
                value: Some("1".into()),
                exported: false,
                readonly: false,
            },
        );
        vars.entry("PATH".into()).or_insert(Variable {
            value: Some("/bin".into()),
            exported: true,
            readonly: false,
        });
        let mut shell = Self {
            shell_pid: unsafe { getpid() },
            substitution_status: None,
            vars,
            functions: BTreeMap::new(),
            function_depth: 0,
            source_depth: 0,
            eval_depth: 0,
            returning: false,
            args,
            last: 0,
            background: Vec::new(),
            jobs: Vec::new(),
            loop_depth: 0,
            flow: None,
            monitor: false,
            interactive: false,
            noglob: false,
            nounset: false,
            exiting: false,
            requested_exit: false,
            regular_builtin: false,
        };
        shell.initialize_directory();
        shell
    }
    fn get(&self, key: &str) -> &str {
        self.variable_value(key).unwrap_or("")
    }
    fn arithmetic(&mut self, word: &Word) -> Result<String, ()> {
        let text = self.expand(word, false)?.concat();
        arithmetic::evaluate(self, &text)
            .map(|n| n.to_string())
            .map_err(|message| {
                diagnostic(&format!("arithmetic: {message}"));
                self.exiting = !self.interactive;
            })
    }
    fn external(
        &self,
        words: &[String],
        assignments: &[(String, String)],
        default_path: bool,
    ) -> i32 {
        let name = &words[0];
        if name.is_empty() {
            diagnostic("empty command name");
            return 127;
        }
        let variables = self
            .vars
            .iter()
            .filter(|(k, v)| v.exported && !assignments.iter().any(|(a, _)| a == *k))
            .filter_map(|(k, v)| v.value.as_ref().map(|value| format!("{k}={value}")))
            .chain(assignments.iter().map(|(k, v)| format!("{k}={v}")));
        let Ok(env) = variables.map(CString::new).collect::<Result<Vec<_>, _>>() else {
            return 126;
        };
        let Ok(argv) = words
            .iter()
            .map(|s| CString::new(s.as_str()))
            .collect::<Result<Vec<_>, _>>()
        else {
            return 126;
        };
        let vector = |values: &[CString]| {
            values
                .iter()
                .map(|s| s.as_ptr())
                .chain(core::iter::once(core::ptr::null()))
                .collect::<Vec<_>>()
        };
        let av = vector(&argv);
        let ev = vector(&env);
        let path = if default_path {
            lookup::DEFAULT_PATH
        } else {
            assignments
                .iter()
                .rev()
                .find(|(k, _)| k == "PATH")
                .map_or(self.get("PATH"), |(_, v)| v)
        };
        let mut denied = false;
        let mut attempt = |filename: &str| -> Option<i32> {
            let file = CString::new(filename).ok()?;
            unsafe {
                execvpe(file.as_ptr(), av.as_ptr(), ev.as_ptr());
            }
            match error() {
                2 | 20 => None,
                // A PATH entry may be on a filesystem with a shorter name limit
                // than a later entry. This candidate did not resolve to a file.
                36 if !name.contains('/') => None,
                13 => {
                    denied = true;
                    None
                }
                e => {
                    diagnostic(&format!("{filename}: exec error {e}"));
                    Some(126)
                }
            }
        };
        if name.contains('/') {
            if let Some(code) = attempt(name) {
                return code;
            }
        } else {
            for dir in path.split(':') {
                let candidate = if dir.is_empty() {
                    format!("./{name}")
                } else {
                    format!("{dir}/{name}")
                };
                if let Some(code) = attempt(&candidate) {
                    return code;
                }
            }
        }
        diagnostic(&format!(
            "{name}: {}",
            if denied {
                "permission denied"
            } else {
                "command not found"
            }
        ));
        if denied { 126 } else { 127 }
    }
    fn command_kind(&self, name: &str, functions: bool) -> CommandKind {
        match name {
            "." | ":" | "break" | "continue" | "eval" | "exec" | "exit" | "export" | "readonly"
            | "return" | "set" | "shift" | "unset" => CommandKind::Special,
            _ if functions && self.functions.contains_key(name) => CommandKind::Function,
            "true" | "false" | "cd" | "pwd" | "read" | "getopts" | "wait" | "jobs" | "fg"
            | "bg" | "command" => CommandKind::Builtin,
            _ => CommandKind::External,
        }
    }

    fn prefix(
        &mut self,
        assignments: Vec<(String, String)>,
    ) -> Result<Vec<(String, Option<Variable>)>, ()> {
        for (name, _) in &assignments {
            self.check_variable(name)?;
        }
        Ok(assignments
            .into_iter()
            .map(|(name, value)| {
                let old = self.vars.insert(
                    name.clone(),
                    Variable {
                        value: Some(value),
                        exported: true,
                        readonly: false,
                    },
                );
                (name, old)
            })
            .collect())
    }

    fn restore_prefix(&mut self, saved: Vec<(String, Option<Variable>)>) {
        for (name, old) in saved.into_iter().rev() {
            if let Some(var) = old {
                self.vars.insert(name, var);
            } else {
                self.vars.remove(&name);
            }
        }
    }

    fn builtin(&mut self, words: &[String]) -> Option<i32> {
        let name = words.first()?.as_str();
        let args = &words[1..];
        let code = match name {
            ":" | "true" => 0,
            "false" => 1,
            "." => self.dot(args),
            "eval" => self.eval(args),
            "read" => self.read_variables(args),
            "getopts" => self.get_options(args),
            "set" => self.set_arguments(args),
            "shift" => self.shift(args),
            "return" => {
                if self.function_depth + self.source_depth == 0 || args.len() > 1 {
                    diagnostic("return: expected a function or dot script and at most one status");
                    return Some(2);
                }
                let code = match args.first() {
                    Some(v) => v.parse::<i64>().map_or(2, |n| (n & 255) as i32),
                    None => self.last,
                };
                self.returning = true;
                code
            }
            "break" | "continue" => self.loop_control(name == "break", args),
            "exit" => {
                if args.len() > 1 {
                    diagnostic("exit: too many arguments");
                    return Some(1);
                }
                self.exiting = true;
                self.requested_exit = true;
                args.first().map_or(self.last, |s| {
                    s.parse::<i64>().map_or(2, |n| (n & 255) as i32)
                })
            }
            "export" | "readonly" => self.declare_variables(name, args),
            "unset" => self.unset(args),
            "cd" => self.cd(args),
            "pwd" => self.pwd(args),
            "jobs" => {
                for j in &mut self.jobs {
                    j.refresh(false);
                    j.report();
                }
                self.jobs.retain(|j| !j.pids.is_empty());
                0
            }
            "fg" | "bg" => {
                let Some(i) = jobs::select(&self.jobs, args.first()) else {
                    diagnostic("no such job");
                    return Some(1);
                };
                if name == "bg" {
                    i32::from(!self.jobs[i].resume())
                } else {
                    let result = self.jobs[i].foreground();
                    if self.jobs[i].pids.is_empty() {
                        self.jobs.remove(i);
                    }
                    result
                }
            }
            "wait" => {
                if args.is_empty() {
                    for job in &mut self.jobs {
                        job.refresh(true);
                    }
                    self.jobs.retain(|j| !j.pids.is_empty());
                    for pid in core::mem::take(&mut self.background) {
                        let _ = wait(pid);
                    }
                    0
                } else {
                    let mut last = 0;
                    for s in args {
                        let Some(pid) = s.parse::<i32>().ok().filter(|n| *n > 0) else {
                            return Some(127);
                        };
                        last = wait(pid).unwrap_or(127);
                        self.background.retain(|n| *n != pid);
                    }
                    last
                }
            }
            "exec" if !args.is_empty() => {
                let status = self.external(args, &[], false);
                self.special_failure();
                status
            }
            "exec" => 0,
            _ => return None,
        };
        Some(code)
    }

    fn invocation(&mut self, command: &syntax::Command) -> Result<Invocation, ()> {
        self.substitution_status = None;
        let mut assignments: Vec<(String, String)> = Vec::new();
        let mut words = Vec::new();
        for word in &command.words {
            if (words.is_empty() || self.declaration(&words))
                && let Some(Part::Literal(first)) = word.first()
                && let Some((key, _)) = first.split_once('=')
                && syntax::identifier(key)
            {
                let assigned = self.assignment(word)?;
                if words.is_empty() {
                    let (_, val) = assigned.split_once('=').unwrap();
                    assignments.push((key.into(), val.into()));
                } else {
                    words.push(assigned);
                }
                continue;
            }
            words.extend(self.expand(word, true)?);
        }
        // Validate in the parent shell, including assignments before an external
        // command. Detecting a readonly error only after fork would let a script
        // incorrectly continue executing its next command.
        for (name, _) in &assignments {
            if self.check_variable(name).is_err() {
                self.exiting = !self.interactive;
                return Err(());
            }
        }
        let action = self.resolve(&words);
        Ok(Invocation {
            words,
            assignments,
            action,
        })
    }
    fn command(&mut self, command: &syntax::Command, child: bool) -> i32 {
        match self.invocation(command) {
            Ok(invocation) => self.invoke(command, child, invocation),
            Err(()) => 1,
        }
    }
    fn invoke(&mut self, command: &syntax::Command, child: bool, invocation: Invocation) -> i32 {
        let Invocation {
            words,
            assignments,
            action,
        } = invocation;
        let permanent = action.permanent_redirect(&words);
        let saved = match redirection::apply(self, &command.redirects, !child) {
            Ok(saved) => saved,
            Err(()) => {
                if action.special() {
                    self.exiting = !self.interactive;
                }
                return 1;
            }
        };
        let result = if let Some(compound) = &command.compound {
            self.compound(compound)
        } else if words.is_empty() {
            let mut status = self.substitution_status.unwrap_or(0);
            for (k, v) in &assignments {
                if self.set(k, v).is_err() {
                    self.exiting = !self.interactive;
                    status = 1;
                    break;
                }
            }
            status
        } else {
            match action {
                lookup::Action::Execute {
                    first,
                    kind,
                    wrapped,
                    default_path,
                } => {
                    let words = &words[first..];
                    match kind {
                        CommandKind::External => self.external(words, &assignments, default_path),
                        CommandKind::Function => {
                            let body = self.functions[&words[0]].clone();
                            self.call_function(&body, words, assignments)
                        }
                        CommandKind::Builtin | CommandKind::Special => {
                            match self.prefix(assignments) {
                                Ok(old) => {
                                    let regular = kind == CommandKind::Builtin || wrapped;
                                    let previous =
                                        core::mem::replace(&mut self.regular_builtin, regular);
                                    let status = self.builtin(words).unwrap();
                                    self.regular_builtin = previous;
                                    // An error in eval/dot stops that evaluation,
                                    // but command suppresses the special builtin's
                                    // abort of its caller. An explicit exit still
                                    // leaves the shell, including from a script.
                                    if wrapped && !self.requested_exit {
                                        self.exiting = false;
                                    }
                                    if regular {
                                        self.restore_prefix(old);
                                    }
                                    status
                                }
                                Err(()) => {
                                    self.exiting = !self.interactive;
                                    1
                                }
                            }
                        }
                    }
                }
                lookup::Action::Describe {
                    first,
                    verbose,
                    default_path,
                } => match self.prefix(assignments) {
                    Ok(old) => {
                        let status = self.describe(&words[first..], verbose, default_path);
                        self.restore_prefix(old);
                        status
                    }
                    Err(()) => 1,
                },
                lookup::Action::Empty => 0,
                lookup::Action::Invalid => {
                    diagnostic("command: invalid options or missing query operand");
                    127
                }
            }
        };
        redirection::finish(saved, !permanent);
        result
    }
    fn pipeline(&mut self, p: &Pipeline) -> i32 {
        let mut prepared = None;
        if p.commands.len() == 1 && !p.background {
            let Ok(call) = self.invocation(&p.commands[0]) else {
                return 1;
            };
            if !matches!(p.commands[0].compound, Some(syntax::Compound::Subshell(_)))
                && !call.action.external()
            {
                return self.invoke(&p.commands[0], false, call);
            }
            prepared = Some(call);
        }
        let mut children = Vec::new();
        let mut group = 0;
        let mut input = -1;
        let mut failed = false;
        for (i, cmd) in p.commands.iter().enumerate() {
            let mut next = [-1, -1];
            if i + 1 < p.commands.len() && unsafe { pipe(next.as_mut_ptr()) } != 0 {
                failed = true;
                break;
            }
            let pid = unsafe { fork() };
            if pid == 0 {
                if self.monitor {
                    jobs::child_group(group);
                    self.monitor = false;
                }
                if input >= 0 {
                    if unsafe { dup2(input, 0) } < 0 {
                        exit(1)
                    };
                    let _ = close(input);
                }
                if next[1] >= 0 {
                    if unsafe { dup2(next[1], 1) } < 0 {
                        exit(1)
                    };
                    let _ = close(next[0]);
                    let _ = close(next[1]);
                }
                exit(match prepared.take() {
                    Some(call) => self.invoke(cmd, true, call),
                    None => self.command(cmd, true),
                });
            }
            if input >= 0 {
                let _ = close(input);
            }
            if next[1] >= 0 {
                let _ = close(next[1]);
            }
            input = next[0];
            if pid < 0 {
                failed = true;
                break;
            }
            if self.monitor {
                if group == 0 {
                    group = pid;
                }
                jobs::assign(pid, group);
            }
            children.push(pid);
        }
        if input >= 0 {
            let _ = close(input);
        }
        if self.monitor && !failed {
            let name = p
                .commands
                .first()
                .and_then(|c| c.words.first())
                .and_then(|word| word.first())
                .map(|part| match part {
                    Part::Literal(s) | Part::Quoted(s) | Part::Parameter(s, _) => s.clone(),
                    Part::Braced(name, ..) => name.clone(),
                    Part::Substitution(..) => "$(…)".into(),
                    Part::Arithmetic(..) => "$((…))".into(),
                })
                .unwrap_or_default();
            let Some(mut job) = jobs::Job::new(group, children, name) else {
                return 1;
            };
            if p.background {
                self.jobs.push(job);
                return 0;
            }
            let status = job.foreground();
            if job.stopped {
                job.report();
                self.jobs.push(job);
            }
            return status;
        }
        if p.background && !failed {
            self.background.extend(children);
            return 0;
        }
        let mut status = 0;
        for pid in children {
            status = wait(pid).unwrap_or(127);
        }
        if failed {
            diagnostic("cannot start pipeline");
            1
        } else {
            status
        }
    }
    fn run_list(&mut self, list: &[Pipeline]) -> i32 {
        for p in list {
            if self.exiting || self.returning || self.flow.is_some() {
                break;
            }
            if p.when == Condition::Success && self.last != 0
                || p.when == Condition::Failure && self.last == 0
            {
                continue;
            }
            let status = self.pipeline(p);
            self.last = if p.negate && !self.exiting && !self.returning {
                i32::from(status == 0)
            } else {
                status
            };
        }
        self.last
    }

    fn run(&mut self, text: &str) {
        match syntax::parse(text) {
            Ok(list) if list.is_empty() => self.last = 0,
            Ok(list) => {
                self.run_list(&list);
            }
            Err(e) => {
                diagnostic(e.message());
                self.last = 2;
                self.exiting = !self.interactive;
            }
        }
    }
}

fn cwd() -> Option<String> {
    directory::physical().ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let raw = unsafe { Args::from_raw(argc, argv) };
    let args: Vec<String> = (0..raw.len())
        .filter_map(|i| raw.get(i))
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let mut env = Vec::new();
    let mut ptr = unsafe { argv.add(raw.len() + 1) };
    while !unsafe { *ptr }.is_null() {
        env.push(
            unsafe { CStr::from_ptr((*ptr).cast()) }
                .to_string_lossy()
                .into_owned(),
        );
        ptr = unsafe { ptr.add(1) };
    }
    let mut sh = Shell::new(
        vec![args.first().cloned().unwrap_or_else(|| "sh".into())],
        &env,
    );
    let arguments = args.get(1..).unwrap_or_default();
    let Ok(options) = sh.options(arguments, true) else {
        return 2;
    };
    let operands = &arguments[options.first..];
    if options.command {
        let Some((text, positional)) = operands.split_first() else {
            diagnostic("-c requires a command");
            return 2;
        };
        if !positional.is_empty() {
            sh.args = positional.to_vec();
        }
        sh.interactive = options.interactive;
        sh.run(text);
        return sh.last;
    }
    let script = if options.stdin {
        None
    } else {
        operands.first()
    };
    let fd = if let Some(path) = script {
        let Ok(c) = CString::new(path.as_str()) else {
            return 2;
        };
        match open(&c, 0).and_then(redirection::script_descriptor) {
            Ok(n) => n,
            Err(e) => {
                diagnostic(&format!("{path}: open error {e}"));
                return 127;
            }
        }
    } else {
        0
    };
    let interactive =
        options.interactive || (script.is_none() && !options.stdin && jobs::interactive());
    if script.is_some() {
        sh.args = operands.to_vec();
    } else {
        sh.args.extend_from_slice(operands);
    }
    sh.interactive = interactive;
    sh.monitor = interactive && jobs::interactive();
    if sh.monitor {
        jobs::ignore_keyboard();
    }
    sh.last = sh.run_input(fd, script.is_some(), interactive);
    if fd != 0 {
        let _ = close(fd);
    }
    sh.last
}
