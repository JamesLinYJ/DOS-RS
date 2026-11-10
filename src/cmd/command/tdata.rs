//! Internal command names from `CMD/COMMAND/TDATA.ASM` `COMTAB`.
//!
//! The first byte in the original table is the name length. Here the name
//! slice carries that length. The flag byte is `fCheckDrive` in bit 0 and
//! `fSwitchAllowed` in bit 1. A zero name ends the table.

pub const F_CHECK_DRIVE: u8 = 0b0000_0001;
pub const F_SWITCH_ALLOWED: u8 = 0b0000_0010;

pub const COMTAB: &[(&[u8], u8)] = &[
    (b"DIR", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"CALL", F_SWITCH_ALLOWED),
    (b"CHCP", F_SWITCH_ALLOWED),
    (b"RENAME", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"REN", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"ERASE", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"DEL", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"TYPE", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"REM", F_SWITCH_ALLOWED),
    (b"COPY", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"PAUSE", F_SWITCH_ALLOWED),
    (b"DATE", F_SWITCH_ALLOWED),
    (b"TIME", F_SWITCH_ALLOWED),
    (b"VER", 0),
    (b"VOL", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"CD", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"CHDIR", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"MD", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"MKDIR", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"RD", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"RMDIR", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
    (b"BREAK", F_SWITCH_ALLOWED),
    (b"VERIFY", F_SWITCH_ALLOWED),
    (b"SET", F_SWITCH_ALLOWED),
    (b"PROMPT", F_SWITCH_ALLOWED),
    (b"PATH", F_SWITCH_ALLOWED),
    (b"EXIT", 0),
    (b"CTTY", F_CHECK_DRIVE | F_SWITCH_ALLOWED),
    (b"ECHO", F_SWITCH_ALLOWED),
    (b"GOTO", F_SWITCH_ALLOWED),
    (b"SHIFT", F_SWITCH_ALLOWED),
    (b"IF", F_SWITCH_ALLOWED),
    (b"FOR", F_SWITCH_ALLOWED),
    (b"CLS", 0),
    (b"ATTRIB", F_SWITCH_ALLOWED),
    (b"TRUENAME", F_SWITCH_ALLOWED | F_CHECK_DRIVE),
];

/// Message 1040. Thirteen spaces sit between `DOS` and `Version`.
pub const VER_MESSAGE: &[u8] = b"Microsoft DOS             Version ";
