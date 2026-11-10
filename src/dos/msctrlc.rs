//! `CNTCHAND` from `DOS/MSCTRLC.ASM`.
//!
//! The IBM wrapper includes `MSSW.ASM`. Unless the raw-output break flag is
//! set, the handler displays three bytes and a new line. A return on the
//! same stack retries the call. A return on a different stack pops the
//! flags: carry clear still retries, and carry set aborts with `DidCTRLC`
//! set to all ones.

const IBM: u16 = 0xffff;
const MESSAGE_LENGTH: u8 = 3;

pub struct Reply {
    pub printed: u8,
    pub retry: bool,
    pub ctrl_c: u8,
}

/// Handle one Ctrl-C return from the user.
pub fn ctrl_c(break_flag: bool, stack_matches: bool, carry: bool) -> Reply {
    let printed = if break_flag { 0 } else { MESSAGE_LENGTH };
    if stack_matches || !carry {
        return Reply {
            printed,
            retry: true,
            ctrl_c: 0,
        };
    }
    Reply {
        printed,
        retry: false,
        ctrl_c: 0xff,
    }
}

pub fn show() {
    let retry = ctrl_c(false, true, true);
    let abort = ctrl_c(false, false, true);
    crate::bios::mscon::con_write_bytes(b"MSCTRLC IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" MSG");
    put_hex8(retry.printed);
    crate::bios::mscon::con_write_bytes(if retry.retry { b" RT00" } else { b" RTFF" });
    crate::bios::mscon::con_write_bytes(if abort.retry { b" AB00" } else { b" AB" });
    if !abort.retry {
        put_hex8(abort.ctrl_c);
    }
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
