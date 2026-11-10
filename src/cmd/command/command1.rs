//! Resident command control from `CMD/COMMAND/COMMAND1.ASM`.
//!
//! An EXEC error is reported in a fixed order: missing file, out of memory,
//! bad format, access denied, then the generic failure. Ctrl-C is ignored
//! during initialization, becomes an empty answer during the date prompt, and
//! aborts a nested call only for functions 1 through 12.

const FILE_NOT_FOUND: u16 = 2;
const ACCESS_DENIED: u16 = 5;
const NOT_ENOUGH_MEMORY: u16 = 8;
const BAD_FORMAT: u16 = 11;
const BAD_NAME: u16 = 229;
const TOO_BIG: u16 = 227;
const BAD_EXE: u16 = 226;
const DENIED: u16 = 230;
const EXEC_FAILED: u16 = 225;
const INIT_RUNNING: u8 = 0x01;
const INIT_SPECIAL: u8 = 0x02;
const IN_CTRL_C: u8 = 0x04;

#[derive(Clone, Copy)]
struct Session {
    init_flag: u8,
    single: u16,
    batch: u16,
    previous: u16,
    nest: u8,
    echo: u8,
    parent_echo: u8,
    pipe: u8,
    external: u8,
    for_segment: u16,
    shell_keeps: bool,
    user_ends: bool,
    reset_disks: bool,
}

#[derive(Clone, Copy)]
struct CtrlC {
    kind: u8,
    echo: u8,
    pipe: u8,
    nest: u8,
    single: u16,
    batch: u16,
    for_segment: u16,
    reset_disks: bool,
}

fn exec_message(error: u16) -> u16 {
    if error == FILE_NOT_FOUND {
        return BAD_NAME;
    }
    if error == NOT_ENOUGH_MEMORY {
        return TOO_BIG;
    }
    if error == BAD_FORMAT {
        return BAD_EXE;
    }
    if error == ACCESS_DENIED {
        return DENIED;
    }
    EXEC_FAILED
}

fn finish(session: &mut Session) -> u8 {
    if session.pipe != 0 {
        session.echo >>= 1;
    }
    session.pipe = 0;
    if session.single != 0 {
        session.single = 0xFFFF;
    }
    session.init_flag &= !IN_CTRL_C;
    if session.external == 0 {
        3
    } else {
        4
    }
}

fn end_batch(session: &mut Session) {
    let mut echo = session.echo;
    loop {
        if session.shell_keeps {
            break;
        }
        session.for_segment = 0;
        echo = session.parent_echo;
        session.batch = session.previous;
        session.nest = session.nest.wrapping_sub(1);
        if session.nest == 0 {
            break;
        }
    }
    session.echo = echo;
    session.pipe = 0;
}

/// `CONTC`. `function` is the DOS call that was in progress.
fn ctrl_c(mut session: Session, function: u8) -> CtrlC {
    if session.init_flag & INIT_RUNNING != 0 {
        let kind = if session.init_flag & INIT_SPECIAL != 0 {
            1
        } else {
            0
        };
        return snapshot(kind, &session);
    }
    if session.init_flag & IN_CTRL_C != 0 {
        let kind = if !(1..=12).contains(&function) { 0 } else { 2 };
        return snapshot(kind, &session);
    }
    session.init_flag |= IN_CTRL_C;
    session.reset_disks = session.single == 0;
    if session.batch != 0 && session.single == 0 && session.user_ends {
        end_batch(&mut session);
    }
    let kind = finish(&mut session);
    snapshot(kind, &session)
}

fn snapshot(kind: u8, session: &Session) -> CtrlC {
    CtrlC {
        kind,
        echo: session.echo,
        pipe: session.pipe,
        nest: session.nest,
        single: session.single,
        batch: session.batch,
        for_segment: session.for_segment,
        reset_disks: session.reset_disks,
    }
}

fn blank() -> Session {
    Session {
        init_flag: 0,
        single: 0,
        batch: 0,
        previous: 0,
        nest: 0,
        echo: 0,
        parent_echo: 0,
        pipe: 0,
        external: 0,
        for_segment: 0,
        shell_keeps: false,
        user_ends: false,
        reset_disks: false,
    }
}

pub fn show() {
    let mut init = blank();
    init.init_flag = INIT_RUNNING;
    let ignored = ctrl_c(init, 1);
    init.init_flag = INIT_RUNNING | INIT_SPECIAL;
    let special = ctrl_c(init, 1);
    let mut nested = blank();
    nested.init_flag = IN_CTRL_C;
    let nested_zero = ctrl_c(nested, 0);
    let nested_one = ctrl_c(nested, 1);
    let nested_twelve = ctrl_c(nested, 12);
    let nested_thirteen = ctrl_c(nested, 13);
    let reload = ctrl_c(blank(), 0);
    let mut external = blank();
    external.external = 1;
    let abort = ctrl_c(external, 0);
    let mut piped = blank();
    piped.pipe = 1;
    piped.echo = 3;
    let shifted = ctrl_c(piped, 0);
    let mut kept = blank();
    kept.batch = 0x0100;
    kept.echo = 4;
    let keep = ctrl_c(kept, 0);
    let mut ending = blank();
    ending.batch = 0x0100;
    ending.nest = 1;
    ending.parent_echo = 5;
    ending.user_ends = true;
    ending.external = 1;
    ending.for_segment = 0x0010;
    let freed = ctrl_c(ending, 0);
    let mut shell = blank();
    shell.batch = 0x0200;
    shell.nest = 2;
    shell.echo = 7;
    shell.user_ends = true;
    shell.shell_keeps = true;
    let kept_by_shell = ctrl_c(shell, 0);
    let mut once = blank();
    once.single = 0x0081;
    let single = ctrl_c(once, 0);

    crate::bios::mscon::con_write_bytes(b"CMD1 EX");
    put_hex8(FILE_NOT_FOUND as u8);
    put_hex8(exec_message(FILE_NOT_FOUND) as u8);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(NOT_ENOUGH_MEMORY as u8);
    put_hex8(exec_message(NOT_ENOUGH_MEMORY) as u8);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(BAD_FORMAT as u8);
    put_hex8(exec_message(BAD_FORMAT) as u8);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(ACCESS_DENIED as u8);
    put_hex8(exec_message(ACCESS_DENIED) as u8);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(1);
    put_hex8(exec_message(1) as u8);
    write_ctrl(b" K", &ignored);
    write_ctrl(b" K", &special);
    write_ctrl(b" K", &nested_zero);
    write_ctrl(b" K", &nested_one);
    write_ctrl(b" K", &nested_twelve);
    write_ctrl(b" K", &nested_thirteen);
    write_ctrl(b" K", &reload);
    write_ctrl(b" K", &abort);
    write_ctrl(b" K", &shifted);
    write_ctrl(b" K", &keep);
    write_ctrl(b" K", &freed);
    write_ctrl(b" K", &kept_by_shell);
    write_ctrl(b" K", &single);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_ctrl(label: &[u8], result: &CtrlC) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(result.kind);
    put_hex8(result.echo);
    put_hex8(result.pipe);
    put_hex8(result.nest);
    put_hex16(result.single);
    put_hex16(result.batch);
    put_hex16(result.for_segment);
    put_hex8(u8::from(result.reset_disks));
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
