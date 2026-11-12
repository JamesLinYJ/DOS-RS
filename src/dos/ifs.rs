//! `IFS_DOSCALL` and `GET_DOS_INFO` from `DOS/IFS.ASM`.
//!
//! Function 39 enters DOS and function 40 leaves it. Function 38 reports the
//! I/O stack size. Function 32 with code 8 returns the share retry count.
//! Functions 36 and 37 compare and copy paths, folding case and treating `/`
//! as `\`. Any other function fails and still leaves the in-DOS count as it
//! found it.

const ENTER_DOS: u8 = 39;
const LEAVE_DOS: u8 = 40;
const STACK_QUERY: u8 = 38;
const DOS_INFO: u8 = 32;
const COMPARE: u8 = 36;
const COPY: u8 = 37;
const STACK_BYTES: u16 = 0x180;
const RETRY_COUNT: u16 = 3;
const INFO_RETRY: u8 = 8;

use core::sync::atomic::{AtomicU8, Ordering};

static IN_DOS: AtomicU8 = AtomicU8::new(0);

pub struct Answer {
    pub in_dos: u8,
    pub stack_bytes: u16,
    pub retry: u16,
    pub failed: bool,
    pub same: bool,
}

/// One IFS service request. `info` is used by function 32.
pub fn dos_call(function: u8, info: u8, left: &[u8], right: &mut [u8]) -> Answer {
    if function == ENTER_DOS {
        IN_DOS.fetch_add(1, Ordering::Relaxed);
        return answer(false, false);
    }
    if function == LEAVE_DOS {
        IN_DOS.fetch_sub(1, Ordering::Relaxed);
        return answer(false, false);
    }
    if function == STACK_QUERY {
        let mut reply = answer(false, false);
        reply.stack_bytes = STACK_BYTES;
        return reply;
    }
    IN_DOS.fetch_add(1, Ordering::Relaxed);
    let mut failed = false;
    let mut same = false;
    let mut retry = 0u16;
    if function == DOS_INFO {
        match info {
            INFO_RETRY => retry = RETRY_COUNT,
            _ => failed = true,
        }
    } else if function == COMPARE {
        same = same_path(left, right);
    } else if function == COPY {
        copy_path(left, right);
    } else {
        failed = true;
    }
    IN_DOS.fetch_sub(1, Ordering::Relaxed);
    let mut reply = answer(failed, same);
    reply.retry = retry;
    reply
}

fn answer(failed: bool, same: bool) -> Answer {
    Answer {
        in_dos: IN_DOS.load(Ordering::Relaxed),
        stack_bytes: 0,
        retry: 0,
        failed,
        same,
    }
}

/// `StrCmp`. Case is ignored and `/` matches `\`.
pub fn same_path(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0usize;
    loop {
        let from_left = fold(at(left, index));
        let from_right = fold(at(right, index));
        if from_left != from_right {
            return false;
        }
        if from_left == 0 {
            return true;
        }
        index += 1;
    }
}

/// `StrCpy`. The copy is uppercase and `/` becomes `\`.
pub fn copy_path(source: &[u8], destination: &mut [u8]) {
    let mut index = 0usize;
    while index < destination.len() {
        let byte = fold(at(source, index));
        destination[index] = byte;
        if byte == 0 {
            break;
        }
        index += 1;
    }
}

fn fold(byte: u8) -> u8 {
    let upper = byte.to_ascii_uppercase();
    if upper == b'/' { b'\\' } else { upper }
}

fn at(text: &[u8], index: usize) -> u8 {
    if index < text.len() { text[index] } else { 0 }
}

pub fn show() {
    let entered = dos_call(ENTER_DOS, 0, b"", &mut []);
    let left = dos_call(LEAVE_DOS, 0, b"", &mut []);
    let stack = dos_call(STACK_QUERY, 0, b"", &mut []);
    let retry = dos_call(DOS_INFO, INFO_RETRY, b"", &mut []);
    let failed = dos_call(99, 0, b"", &mut []);
    let mut other = *b"a:\\\0";
    let compared = dos_call(COMPARE, 0, b"A:/\0", &mut other);
    let mut copied = [0u8; 8];
    copy_path(b"a:/dos\0", &mut copied);
    crate::bios::mscon::con_write_bytes(b"IFS IN");
    put_hex8(entered.in_dos);
    crate::bios::mscon::con_write_bytes(b" OUT");
    put_hex8(left.in_dos);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(stack.stack_bytes);
    crate::bios::mscon::con_write_bytes(b" RT");
    put_hex16(retry.retry);
    crate::bios::mscon::con_write_bytes(if failed.failed { b" ERCF" } else { b" EROK" });
    crate::bios::mscon::con_write_bytes(if compared.same { b" EQ " } else { b" NE " });
    let mut end = 0usize;
    while end < copied.len() && copied[end] != 0 {
        end += 1;
    }
    crate::bios::mscon::con_write_bytes(&copied[..end]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
