#![no_std]
extern crate alloc;
mod compound;
mod jobs;
mod redirection;
mod substitution;
mod syntax;

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
    fn execve(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32;
    fn waitpid(pid: i32, status: *mut i32, flags: i32) -> i32;
    fn getpid() -> i32;
    fn dup2(old: i32, new: i32) -> i32;
    fn pipe(fds: *mut i32) -> i32;
    fn sys_chdir(path: *const i8) -> isize;
    fn sys_getcwd(buf: *mut u8, size: usize) -> isize;
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
    value: String,
    exported: bool,
}

struct Invocation {
    words: Vec<String>,
    assignments: Vec<(String, String)>,
}

struct Shell {
    shell_pid: i32,
    substitution_status: Option<i32>,
    vars: BTreeMap<String, Variable>,
    functions: BTreeMap<String, alloc::rc::Rc<syntax::Command>>,
    function_depth: usize,
    returning: bool,
    args: Vec<String>,
    last: i32,
    background: Vec<i32>,
    jobs: Vec<jobs::Job>,
    loop_depth: usize,
    flow: Option<compound::LoopControl>,
    monitor: bool,
    exiting: bool,
}
impl Shell {
    fn new(args: Vec<String>, env: &[String]) -> Self {
        let mut vars = BTreeMap::new();
        for s in env {
            if let Some((name, val)) = s.split_once('=') {
                vars.insert(
                    name.into(),
                    Variable {
                        value: val.into(),
                        exported: true,
                    },
                );
            }
        }
        vars.entry("PATH".into()).or_insert(Variable {
            value: "/bin".into(),
            exported: true,
        });
        Self {
            shell_pid: unsafe { getpid() },
            substitution_status: None,
            vars,
            functions: BTreeMap::new(),
            function_depth: 0,
            returning: false,
            args,
            last: 0,
            background: Vec::new(),
            jobs: Vec::new(),
            loop_depth: 0,
            flow: None,
            monitor: false,
            exiting: false,
        }
    }
    fn get(&self, key: &str) -> &str {
        self.vars.get(key).map_or("", |v| &v.value)
    }
    fn parameter(&self, key: &str) -> String {
        match key {
            "?" => self.last.to_string(),
            "$" => self.shell_pid.to_string(),
            "#" => self.args.len().saturating_sub(1).to_string(),
            "!" => self
                .jobs
                .last()
                .map(jobs::Job::last_pid)
                .or_else(|| self.background.last().copied())
                .map_or_else(String::new, |n| n.to_string()),
            "*" | "@" => self.args.get(1..).unwrap_or_default().join(" "),
            name => {
                if let Ok(i) = name.parse::<usize>() {
                    self.args.get(i).cloned().unwrap_or_default()
                } else {
                    self.get(name).into()
                }
            }
        }
    }
    fn expand(&mut self, word: &Word, split: bool) -> Result<Vec<String>, ()> {
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut present = false;
        for part in word {
            match part {
                Part::Literal(s) | Part::Quoted(s) => {
                    field.push_str(s);
                    present = true;
                }
                Part::Parameter(name, quoted) if name == "@" && *quoted => {
                    for (i, text) in self.args.iter().skip(1).enumerate() {
                        if i > 0 {
                            fields.push(core::mem::take(&mut field));
                        }
                        field.push_str(text);
                        present = true;
                    }
                }
                Part::Parameter(_, quoted) | Part::Substitution(_, quoted) => {
                    let text = match part {
                        Part::Parameter(name, _) => self.parameter(name),
                        Part::Substitution(body, _) => self.capture(body)?,
                        _ => unreachable!(),
                    };
                    if *quoted || !split {
                        field.push_str(&text);
                        present = true;
                        continue;
                    }
                    let ifs = self.vars.get("IFS").map_or(" \t\n", |v| v.value.as_str());
                    for ch in text.chars() {
                        if ifs.contains(ch) {
                            if present {
                                fields.push(core::mem::take(&mut field));
                                present = false;
                            }
                        } else {
                            field.push(ch);
                            present = true;
                        }
                    }
                }
            }
        }
        if present {
            fields.push(field);
        }
        Ok(fields)
    }
    fn set(&mut self, name: &str, value: &str) {
        self.vars.entry(name.into()).or_default().value = value.into();
    }
    fn external(&self, words: &[String], assignments: &[(String, String)]) -> i32 {
        let name = &words[0];
        let variables = self
            .vars
            .iter()
            .filter(|(k, v)| v.exported && !assignments.iter().any(|(a, _)| a == *k))
            .map(|(k, v)| format!("{k}={}", v.value))
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
        let path = assignments
            .iter()
            .rev()
            .find(|(k, _)| k == "PATH")
            .map_or(self.get("PATH"), |(_, v)| v);
        let mut denied = false;
        let mut attempt = |filename: &str| -> Option<i32> {
            let file = CString::new(filename).ok()?;
            unsafe {
                execve(file.as_ptr(), av.as_ptr(), ev.as_ptr());
            }
            match error() {
                2 | 20 => None,
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
                    name.clone()
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
    fn builtin(&mut self, words: &[String]) -> Option<i32> {
        let name = words.first()?.as_str();
        let args = &words[1..];
        let code = match name {
            ":" | "true" => 0,
            "false" => 1,
            "return" => {
                if self.function_depth == 0 || args.len() > 1 {
                    diagnostic("return: expected a function and at most one status");
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
                args.first().map_or(self.last, |s| {
                    s.parse::<i64>().map_or(2, |n| (n & 255) as i32)
                })
            }
            "export" => {
                for arg in args {
                    let (name, val) = arg
                        .split_once('=')
                        .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
                    if !syntax::identifier(name) {
                        diagnostic("invalid variable name");
                        return Some(1);
                    }
                    let var = self.vars.entry(name.into()).or_default();
                    var.exported = true;
                    if let Some(v) = val {
                        var.value = v.into();
                    }
                }
                0
            }
            "unset" => {
                for k in args {
                    self.vars.remove(k);
                }
                0
            }
            "cd" => {
                let dest = args.first().map_or(self.get("HOME"), String::as_str);
                let Ok(path) = CString::new(dest) else {
                    return Some(1);
                };
                if dest.is_empty() || unsafe { sys_chdir(path.as_ptr()) } < 0 {
                    diagnostic(&format!("cd: {dest}: cannot change directory"));
                    1
                } else {
                    if let Some(cwd) = cwd() {
                        self.set("PWD", &cwd);
                    }
                    0
                }
            }
            "pwd" => {
                if let Some(path) = cwd() {
                    let _ = write_all(1, format!("{path}\n").as_bytes());
                    0
                } else {
                    1
                }
            }
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
            "exec" if !args.is_empty() => self.external(args, &[]),
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
            if words.is_empty()
                && let Some(Part::Literal(first)) = word.first()
                && let Some((key, _)) = first.split_once('=')
                && syntax::identifier(key)
            {
                let s = self.expand(word, false)?.concat();
                if let Some((k, v)) = s.split_once('=') {
                    assignments.push((k.into(), v.into()));
                    continue;
                }
            }
            words.extend(self.expand(word, true)?);
        }
        Ok(Invocation { words, assignments })
    }
    fn command(&mut self, command: &syntax::Command, child: bool) -> i32 {
        match self.invocation(command) {
            Ok(invocation) => self.invoke(command, child, invocation),
            Err(()) => 1,
        }
    }
    fn invoke(&mut self, command: &syntax::Command, child: bool, invocation: Invocation) -> i32 {
        let Invocation { words, assignments } = invocation;
        let permanent = words.as_slice() == ["exec"];
        let saved = match redirection::apply(self, &command.redirects, !child) {
            Ok(saved) => saved,
            Err(()) => return 1,
        };
        let result = if let Some(compound) = &command.compound {
            self.compound(compound)
        } else if words.is_empty() {
            for (k, v) in &assignments {
                self.set(k, v);
            }
            self.substitution_status.unwrap_or(0)
        } else if let Some(body) = self.functions.get(&words[0]).cloned() {
            self.call_function(&body, &words, assignments)
        } else if let Some(code) = self.builtin(&words) {
            code
        } else {
            self.external(&words, &assignments)
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
            let name = call.words.first().map(String::as_str);
            if !matches!(p.commands[0].compound, Some(syntax::Compound::Subshell(_)))
                && name.is_none_or(|name| {
                    self.functions.contains_key(name)
                        || matches!(
                            name,
                            ":" | "true"
                                | "false"
                                | "break"
                                | "continue"
                                | "return"
                                | "exit"
                                | "export"
                                | "unset"
                                | "cd"
                                | "pwd"
                                | "wait"
                                | "exec"
                                | "fg"
                                | "bg"
                                | "jobs"
                        )
                })
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
                    Part::Substitution(..) => "$(…)".into(),
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
            self.last = if p.negate {
                i32::from(status == 0)
            } else {
                status
            };
        }
        self.last
    }

    fn run(&mut self, text: &str) {
        match syntax::parse(text) {
            Ok(list) => {
                self.run_list(&list);
            }
            Err(e) => {
                diagnostic(e.message());
                self.last = 2;
            }
        }
    }
}

fn cwd() -> Option<String> {
    let mut bytes = vec![0; 4096];
    let len = unsafe { sys_getcwd(bytes.as_mut_ptr(), bytes.len()) };
    if len <= 0 {
        return None;
    }
    bytes.truncate(len as usize - 1);
    String::from_utf8(bytes).ok()
}
fn read_line(fd: i32) -> Result<Option<String>, i32> {
    let mut bytes = Vec::new();
    let mut ch = [0];
    loop {
        match read(fd, &mut ch)? {
            0 if bytes.is_empty() => return Ok(None),
            0 => break,
            _ if ch[0] == b'\n' => break,
            _ => bytes.push(ch[0]),
        }
    }
    String::from_utf8(bytes).map(Some).map_err(|_| 22)
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
    if args.get(1).is_some_and(|s| s == "-c") {
        let Some(text) = args.get(2) else {
            diagnostic("-c requires a command");
            return 2;
        };
        let positional = if args.len() > 3 {
            args[3..].to_vec()
        } else {
            vec![args[0].clone()]
        };
        let mut sh = Shell::new(positional, &env);
        sh.run(text);
        return sh.last;
    }
    let script = args
        .get(1)
        .filter(|s| s.as_str() != "-s" && s.as_str() != "-i");
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
    let interactive = args.get(1).is_some_and(|s| s == "-i")
        || script.is_none() && !args.get(1).is_some_and(|s| s == "-s") && jobs::interactive();
    let mut sh = Shell::new(
        if script.is_some() {
            args[1..].to_vec()
        } else {
            args
        },
        &env,
    );
    sh.monitor = interactive && jobs::interactive();
    if sh.monitor {
        jobs::ignore_keyboard();
    }
    let mut pending = String::new();
    while !sh.exiting {
        if interactive {
            let _ = write_all(2, if pending.is_empty() { b"$ " } else { b"> " });
        }
        match read_line(fd) {
            Ok(Some(line)) => {
                pending.push_str(&line);
                match syntax::parse(&pending) {
                    Ok(list) => {
                        sh.run_list(&list);
                        pending.clear();
                    }
                    Err(syntax::Error::Incomplete(_)) => pending.push('\n'),
                    Err(e) => {
                        diagnostic(e.message());
                        sh.last = 2;
                        pending.clear();
                        if !interactive {
                            break;
                        }
                    }
                }
            }
            Ok(None) => {
                if !pending.is_empty() {
                    sh.run(&pending);
                }
                break;
            }
            Err(e) => {
                diagnostic(&format!("read error {e}"));
                return 1;
            }
        }
    }
    if fd != 0 {
        let _ = close(fd);
    }
    sh.last
}
