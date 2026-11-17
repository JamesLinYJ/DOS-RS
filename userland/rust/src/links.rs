//! The single-operation POSIX link and unlink utilities.
extern crate alloc;
use crate::{
    Args,
    cli::{self, Arg},
    errno,
};
use alloc::vec::Vec;
use core::ffi::CStr;

unsafe extern "C" {
    fn link(source: *const i8, destination: *const i8) -> i32;
    fn unlink(name: *const i8) -> i32;
}

fn quoted(message: &[u8], value: &CStr) -> Vec<u8> {
    let mut text = Vec::from(message);
    text.push(b'\'');
    text.extend_from_slice(value.to_bytes());
    text.push(b'\'');
    text
}

pub fn run(args: Args<'_>, remove: bool) -> i32 {
    let cmd: &[u8] = if remove { b"unlink" } else { b"link" };
    for arg in cli::arguments(args) {
        match arg {
            Arg::Short(f) => return cli::bad_option(cmd, &f[..1], false),
            Arg::Long(f) => return cli::bad_option(cmd, f, true),
            _ => (),
        }
    }
    let mut operands = cli::operands(args);
    let Some(source) = operands.next() else {
        return cli::usage_error(cmd, b"missing operand");
    };
    let dest = if remove {
        None
    } else if let Some(p) = operands.next() {
        Some(p)
    } else {
        return cli::usage_error(cmd, &quoted(b"missing operand after ", source));
    };
    if let Some(extra) = operands.next() {
        return cli::usage_error(cmd, &quoted(b"extra operand ", extra));
    }
    let rc = if let Some(to) = dest {
        unsafe { link(source.as_ptr(), to.as_ptr()) }
    } else {
        unsafe { unlink(source.as_ptr()) }
    };
    if rc == 0 {
        return 0;
    }
    let error = errno();
    let label = if let Some(to) = dest {
        let mut message = quoted(b"cannot create link ", to);
        message.extend_from_slice(&quoted(b" to ", source));
        message
    } else {
        quoted(b"cannot unlink ", source)
    };
    cli::diagnostic(cmd, &label, error);
    1
}
