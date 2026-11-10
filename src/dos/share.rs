//! Sharing calls from `DOS/SHARE.ASM`.
//!
//! `Installed` is true, so the check and the close go through `JShare`.
//! Slot 1 is `MFT_enter` and slot 2 is `MFTClose`. Both default to `OKCall`,
//! which clears carry. `SHARE_VIOLATION` marks the operation as a read, allows
//! fail and retry, and retries only when the reply is 1. `ShareEnter` clears
//! the sharing index, tries `RetryCount` times, then asks the user.

const CHECK_SLOT: u8 = 1;
const END_SLOT: u8 = 2;
const ALLOWED_FAIL: u8 = 0x08;
const ALLOWED_RETRY: u8 = 0x10;

pub struct Call {
    pub slot: u8,
    pub accepted: bool,
}

pub struct Violation {
    pub reading: u8,
    pub allowed: u8,
    pub probe: u16,
    pub retry: bool,
}

/// `SHARE_CHECK`. The installed path calls jump-table slot 1.
pub fn share_check(denied: bool) -> Call {
    Call {
        slot: CHECK_SLOT,
        accepted: !denied,
    }
}

/// `SHARE_VIOLATION`. Reply 1 retries. Every other reply fails.
pub fn share_violation(reply: u8) -> Violation {
    Violation {
        reading: 0,
        allowed: ALLOWED_FAIL | ALLOWED_RETRY,
        probe: 1,
        retry: reply == 1,
    }
}

/// `ShareEnd`. The installed path calls jump-table slot 2 and keeps its carry.
pub fn share_end(denied: bool) -> Call {
    Call {
        slot: END_SLOT,
        accepted: !denied,
    }
}

/// `ShareEnter`. The sharing index is cleared before each check.
pub fn share_enter(tries: u16, mut failures: u16, mut user_retries: u16) -> Result<u16, ()> {
    loop {
        if share_round(tries, &mut failures) {
            return Ok(0);
        }
        let reply = if user_retries > 0 {
            user_retries -= 1;
            1
        } else {
            0
        };
        if !share_violation(reply).retry {
            return Err(());
        }
    }
}

pub fn show() {
    let open = share_check(false);
    let denied = share_check(true);
    let closed = share_end(false);
    let close_fail = share_end(true);
    let retry = share_violation(1);
    let ignored = share_violation(0);
    let aborted = share_violation(2);
    let failed = share_violation(3);
    let entered = share_enter(3, 0, 0);
    let tried = share_enter(3, 4, 1);
    let exhausted = share_enter(3, 4, 0);
    let replies_fail = !ignored.retry && !aborted.retry && !failed.retry;
    crate::bios::mscon::con_write_bytes(b"SHARE S");
    put_hex8(open.slot);
    crate::bios::mscon::con_write_bytes(if open.accepted { b" OK DN" } else { b" NO DN" });
    put_hex8(denied.slot);
    crate::bios::mscon::con_write_bytes(if denied.accepted { b" OK S" } else { b" NO S" });
    put_hex8(closed.slot);
    crate::bios::mscon::con_write_bytes(if closed.accepted { b" OK EC" } else { b" NO EC" });
    crate::bios::mscon::con_write_bytes(if close_fail.accepted { b" OK RD" } else { b" NO RD" });
    put_hex8(retry.reading);
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex8(retry.allowed);
    crate::bios::mscon::con_write_bytes(b" CX");
    put_hex16(retry.probe);
    crate::bios::mscon::con_write_bytes(if retry.retry { b" RT01" } else { b" RT00" });
    crate::bios::mscon::con_write_bytes(if replies_fail { b" FL00" } else { b" FLFF" });
    crate::bios::mscon::con_write_bytes(b" MFT");
    match entered {
        Ok(index) => put_hex16(index),
        Err(()) => put_hex16(0xffff),
    }
    crate::bios::mscon::con_write_bytes(if tried.is_ok() { b" TRYOK" } else { b" TRYNO" });
    crate::bios::mscon::con_write_bytes(if exhausted.is_err() { b" FAIL" } else { b" FOK" });
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn share_round(tries: u16, failures: &mut u16) -> bool {
    let mut left = tries;
    loop {
        if *failures == 0 {
            return true;
        }
        *failures -= 1;
        if left == 0 {
            return false;
        }
        left -= 1;
        if left == 0 {
            return false;
        }
    }
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
