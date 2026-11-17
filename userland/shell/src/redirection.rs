//! Apply redirections left to right. Temporary copies for built-ins stay
//! outside the command's descriptor operands and close on exec.
use super::{
    Shell, close, diagnostic, dup2, error, open,
    syntax::{Mode, Redirect},
};
use alloc::{ffi::CString, format, string::String, vec::Vec};

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}
const F_DUPFD_CLOEXEC: i32 = 1030;

pub(super) fn script_descriptor(fd: i32) -> Result<i32, i32> {
    // POSIX applications may explicitly redirect descriptors 0 through 9.
    let copy = unsafe { fcntl(fd, F_DUPFD_CLOEXEC, 10) };
    let result = if copy < 0 { Err(error()) } else { Ok(copy) };
    let _ = close(fd);
    result
}

fn redirect(fd: i32, mode: Mode, value: &str) -> Result<(), i32> {
    if mode == Mode::Duplicate {
        if value == "-" {
            return match close(fd) {
                Err(9) => Ok(()),
                other => other,
            };
        }
        let source = value.parse::<i32>().ok().filter(|v| *v >= 0).ok_or(9)?;
        return if unsafe { dup2(source, fd) } < 0 {
            Err(error())
        } else {
            Ok(())
        };
    }
    let flags = match mode {
        Mode::Read => 0,
        Mode::ReadWrite => 2 | 64,
        Mode::Write => 1 | 64 | 512,
        Mode::Append => 1 | 64 | 1024,
        Mode::Duplicate => unreachable!(),
    };
    let path = CString::new(value).map_err(|_| 22)?;
    let opened = open(&path, flags)?;
    if opened != fd {
        let result = if unsafe { dup2(opened, fd) } < 0 {
            Err(error())
        } else {
            Ok(())
        };
        let _ = close(opened);
        result?;
    }
    Ok(())
}

pub(super) fn apply(
    sh: &Shell,
    redirects: &[Redirect],
    preserve: bool,
) -> Result<Vec<(i32, i32)>, ()> {
    let targets: Vec<String> = redirects
        .iter()
        .map(|r| sh.expand(&r.target, false).concat())
        .collect();
    let mut first_free: i32 = 10;
    for (r, value) in redirects.iter().zip(&targets) {
        let source = if r.mode == Mode::Duplicate {
            value.parse::<i32>().unwrap_or(-1)
        } else {
            -1
        };
        first_free = first_free.max(r.fd.max(source).saturating_add(1));
    }
    let mut saved = Vec::new();
    for (r, value) in redirects.iter().zip(targets) {
        if preserve && !saved.iter().any(|(fd, _)| *fd == r.fd) {
            let old = unsafe { fcntl(r.fd, F_DUPFD_CLOEXEC, first_free) };
            if old < 0 && error() != 9 {
                diagnostic("cannot save descriptor");
                finish(saved, true);
                return Err(());
            }
            saved.push((r.fd, old));
        }
        if let Err(e) = redirect(r.fd, r.mode, &value) {
            diagnostic(&format!("{value}: redirection error {e}"));
            finish(saved, true);
            return Err(());
        }
    }
    Ok(saved)
}

pub(super) fn finish(saved: Vec<(i32, i32)>, restore: bool) {
    for (fd, copy) in saved.into_iter().rev() {
        if copy >= 0 {
            if restore {
                unsafe {
                    dup2(copy, fd);
                }
            }
            let _ = close(copy);
        } else if restore {
            let _ = close(fd);
        }
    }
}
