//! Foreground ownership and child status belong to the shell which spawned the pipeline.
use super::{diagnostic, error};
use alloc::{format, string::String, vec::Vec};
use dosrs_user::write_all;
unsafe extern "C" {
    fn getpgrp() -> i32;
    fn tcgetpgrp(fd: i32) -> i32;
    fn tcsetpgrp(fd: i32, group: i32) -> i32;
    fn setpgid(pid: i32, group: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn waitpid(pid: i32, status: *mut i32, flags: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}

const SHELL_SIGNALS: [i32; 5] = [2, 3, 20, 21, 22];
pub fn interactive() -> bool {
    unsafe { tcgetpgrp(0) >= 0 }
}
pub fn ignore_keyboard() {
    for n in SHELL_SIGNALS {
        unsafe { signal(n, 1) };
    }
}
pub fn child_group(group: i32) {
    unsafe { setpgid(0, group) };
    for n in SHELL_SIGNALS {
        unsafe { signal(n, 0) };
    }
}
pub fn assign(pid: i32, group: i32) {
    unsafe { setpgid(pid, group) };
}
pub struct Job {
    pub group: i32,
    pub pids: Vec<i32>,
    last: i32,
    result: i32,
    pub stopped: bool,
    pub name: String,
}
impl Job {
    pub fn new(group: i32, pids: Vec<i32>, name: String) -> Option<Self> {
        let last = *pids.last()?;
        Some(Self {
            group,
            pids,
            last,
            result: 0,
            stopped: false,
            name,
        })
    }
    pub fn last_pid(&self) -> i32 {
        self.last
    }
    pub fn refresh(&mut self, blocking: bool) -> i32 {
        while !self.pids.is_empty() {
            let mut status = 0;
            let flags = 2 | 8 | i32::from(!blocking);
            let pid = unsafe { waitpid(-self.group, &mut status, flags) };
            if pid == -1 {
                if error() == 4 {
                    continue;
                }
                if error() == 10 {
                    self.pids.clear();
                }
                break;
            }
            if pid == 0 {
                break;
            }
            if status == 0xffff {
                self.stopped = false;
                continue;
            }
            if status & 255 == 127 {
                self.stopped = true;
                return 128 + (status >> 8);
            }
            self.pids.retain(|p| *p != pid);
            if self.last == pid {
                self.result = if status & 127 == 0 {
                    status >> 8
                } else {
                    128 + (status & 127)
                };
            }
        }
        self.result
    }
    pub fn foreground(&mut self) -> i32 {
        if unsafe { tcsetpgrp(0, self.group) } < 0 && error() != 1 {
            diagnostic("cannot take foreground terminal");
        }
        if self.stopped {
            self.resume();
        }
        let status = self.refresh(true);
        unsafe {
            tcsetpgrp(0, getpgrp());
        }
        status
    }
    pub fn resume(&mut self) -> bool {
        let ok = unsafe { kill(-self.group, 18) } == 0;
        if ok {
            self.stopped = false;
        }
        ok
    }
    pub fn report(&self) {
        let state = if self.pids.is_empty() {
            "Done"
        } else if self.stopped {
            "Stopped"
        } else {
            "Running"
        };
        let _ = write_all(
            1,
            format!("[{}] {state} {}\n", self.group, self.name).as_bytes(),
        );
    }
}
pub fn select(jobs: &[Job], spec: Option<&String>) -> Option<usize> {
    match spec.map(String::as_str) {
        None | Some("%+") | Some("%%") => jobs.len().checked_sub(1),
        Some("%-") => jobs.len().checked_sub(2),
        Some(s) => {
            let n = s.trim_start_matches('%').parse::<i32>().ok()?;
            jobs.iter().position(|j| j.group == n || j.last == n)
        }
    }
}
