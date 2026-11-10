//! Drive macros from `DOS/MACRO.ASM`.
//!
//! Functions 7 and 8 mark a CDS in use or clear it. Any other assign function
//! asks the redirector with verb 30. A DPB chain ends at `FFFFh`. `A` is unit
//! 0. A drive past the CDS count is invalid. A visible drive that is spliced
//! is invalid. User functions are get, set, then printer through 5. The machine
//! name is 15 bytes and a trailing zero.

const INVALID_DRIVE: u8 = 15;
const NOT_DOS_DISK: u8 = 26;
const INVALID_FUNCTION: u8 = 1;
const LOCUS_UNKNOWN: u8 = 1;
const IN_USE: u16 = 0x4000;
const NET: u16 = 0x11;

pub struct Dpb {
    pub unit: u8,
}

pub struct Cds {
    pub in_use: bool,
    pub spliced: bool,
    pub device: bool,
}

pub struct Fresh {
    pub flags: u16,
    pub text: [u8; 4],
    pub end: u8,
}

pub enum Assign {
    InUse { enabled: bool },
    Redirect { verb: u16 },
}

pub enum User {
    Get,
    Set,
    Printer { verb: u16 },
    Invalid { error: u8, locus: u8 },
}

pub enum DriveFault {
    Invalid,
    NotDos,
}

/// `$AssignOper`. 7 and 8 update the in-use flag. Everything else is verb 30.
pub fn assign(function: u8, unit: u8, count: u8, device: bool) -> Result<Assign, u8> {
    if function == 7 || function == 8 {
        if unit >= count || !device {
            return Err(INVALID_DRIVE);
        }
        return Ok(Assign::InUse {
            enabled: function == 7,
        });
    }
    Ok(Assign::Redirect {
        verb: (NET << 8) | 30,
    })
}

/// `FIND_DPB`. The chain ends when the link is `FFFFh`.
pub fn find_dpb(chain: &[Dpb], unit: u8) -> bool {
    for entry in chain {
        if entry.unit == unit {
            return true;
        }
    }
    false
}

/// `InitCDS`. `A` is drive 1. A letter past the physical drives stays free.
pub fn init_cds(letter: u8, physical: u8, dpb: bool) -> (Fresh, bool) {
    let number = letter - (b'A' - 1);
    let mut fresh = Fresh {
        flags: 0,
        text: [0; 4],
        end: 0,
    };
    if physical < number {
        return (fresh, true);
    }
    fresh.text = [letter, b':', b'\\', 0];
    fresh.flags = IN_USE;
    fresh.end = 2;
    if !dpb {
        return (fresh, true);
    }
    (fresh, false)
}

/// `GetCDSFromDrv`. The unit must be below the CDS count.
pub fn cds_index(count: u8, unit: u8) -> Result<u8, ()> {
    if unit >= count {
        return Err(());
    }
    Ok(unit)
}

/// `$UserOper`. 0 reads the name, 1 writes it, and 2 through 5 are the printer.
pub fn user_oper(function: u8) -> User {
    if function == 0 {
        return User::Get;
    }
    if function == 1 {
        return User::Set;
    }
    if function <= 5 {
        return User::Printer {
            verb: (NET << 8) | 31,
        };
    }
    User::Invalid {
        error: INVALID_FUNCTION,
        locus: LOCUS_UNKNOWN,
    }
}

/// Copy 15 name bytes and force the 16th to zero.
pub fn copy_name(source: &[u8; 16]) -> [u8; 16] {
    let mut name = [0u8; 16];
    name[..15].copy_from_slice(&source[..15]);
    name
}

/// `GetThisDrv`. Zero selects the current drive. A server call builds a dummy CDS.
pub fn this_drive(
    requested: u8,
    current: u8,
    sharing: bool,
    drives: &[Cds],
    physical: u8,
) -> Result<u8, DriveFault> {
    let mut unit = requested;
    if unit == 0 {
        unit = current.wrapping_add(1);
    }
    unit = unit.wrapping_sub(1);
    if sharing {
        let letter = unit + b'A';
        let (cds, _) = init_cds(letter, physical, true);
        if cds.flags & IN_USE == 0 {
            return Err(DriveFault::Invalid);
        }
        return Ok(unit);
    }
    if unit as usize >= drives.len() {
        return Err(DriveFault::Invalid);
    }
    let cds = &drives[unit as usize];
    if cds.in_use {
        return Ok(unit);
    }
    if cds.device {
        return Err(DriveFault::NotDos);
    }
    Err(DriveFault::Invalid)
}

/// `GetVisDrv`. A spliced drive is not visible.
pub fn visible(cds: &Cds) -> Result<(), u8> {
    if cds.spliced {
        return Err(INVALID_DRIVE);
    }
    Ok(())
}

pub fn show() {
    let marked = assign(7, 0, 2, true);
    let cleared = assign(8, 0, 2, true);
    let empty = assign(7, 0, 2, false);
    let redirected = assign(0, 0, 2, true);
    let chain = [Dpb { unit: 0 }, Dpb { unit: 2 }];
    let root = init_cds(b'A', 2, true);
    let missing = init_cds(b'C', 1, false);
    let user_get = user_oper(0);
    let user_set = user_oper(1);
    let printer = user_oper(5);
    let bad_user = user_oper(6);
    let drives = [Cds {
        in_use: true,
        spliced: false,
        device: true,
    }];
    let idle = [Cds {
        in_use: false,
        spliced: false,
        device: true,
    }];
    let current = this_drive(1, 0, false, &drives, 2);
    let formatted = this_drive(1, 0, false, &idle, 2);
    let spliced = Cds {
        in_use: true,
        spliced: true,
        device: true,
    };
    let plain = Cds {
        in_use: true,
        spliced: false,
        device: true,
    };
    let mut source = [b'A'; 16];
    source[15] = b'X';
    let name = copy_name(&source);
    crate::bios::mscon::con_write_bytes(match marked {
        Ok(Assign::InUse { enabled: true }) => b"MACRO A7USE",
        _ => b"MACRO A7BAD",
    });
    crate::bios::mscon::con_write_bytes(match cleared {
        Ok(Assign::InUse { enabled: false }) => b" A8CLR",
        _ => b" A8BAD",
    });
    crate::bios::mscon::con_write_bytes(b" A7");
    put_hex8(match empty {
        Err(error) => error,
        Ok(_) => 0,
    });
    crate::bios::mscon::con_write_bytes(b" AS");
    match redirected {
        Ok(Assign::Redirect { verb }) => put_hex16(verb),
        _ => put_hex16(0),
    }
    crate::bios::mscon::con_write_bytes(if find_dpb(&chain, 2) { b" DP02" } else { b" DPNO" });
    crate::bios::mscon::con_write_bytes(if find_dpb(&chain, 1) { b" DP01" } else { b" DPNO" });
    crate::bios::mscon::con_write_bytes(b" ICA");
    put_hex16(if root.1 { 0xffff } else { root.0.flags });
    crate::bios::mscon::con_write_bytes(b" IC");
    put_hex16(if missing.1 { missing.0.flags } else { 0xffff });
    crate::bios::mscon::con_write_bytes(match cds_index(2, 0) {
        Ok(unit) => if unit == 0 { b" CD00" } else { b" CDFF" },
        Err(()) => b" CDNO",
    });
    crate::bios::mscon::con_write_bytes(match cds_index(2, 2) {
        Ok(_) => b" CDFF",
        Err(()) => b" CDNO",
    });
    crate::bios::mscon::con_write_bytes(match user_get {
        User::Get => b" U0",
        _ => b" U?",
    });
    crate::bios::mscon::con_write_bytes(match user_set {
        User::Set => b" U1",
        _ => b" U?",
    });
    crate::bios::mscon::con_write_bytes(b" UP");
    match printer {
        User::Printer { verb } => put_hex16(verb),
        _ => put_hex16(0),
    }
    crate::bios::mscon::con_write_bytes(b" U6");
    match bad_user {
        User::Invalid { error, locus } => {
            put_hex8(error);
            crate::bios::mscon::con_write_bytes(b" L");
            put_hex8(locus);
        }
        _ => crate::bios::mscon::con_write_bytes(b"00 L00"),
    }
    crate::bios::mscon::con_write_bytes(b" V");
    put_hex8(current.unwrap_or(0xff));
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(match formatted {
        Err(DriveFault::NotDos) => NOT_DOS_DISK,
        Err(DriveFault::Invalid) => INVALID_DRIVE,
        Ok(_) => 0,
    });
    crate::bios::mscon::con_write_bytes(match visible(&spliced) {
        Err(error) if error == INVALID_DRIVE => b" SP0F",
        _ => b" SPFF",
    });
    crate::bios::mscon::con_write_bytes(match visible(&plain) {
        Ok(()) => b" SPOK",
        Err(_) => b" SPNO",
    });
    crate::bios::mscon::con_write_bytes(if name[15] == 0 { b" NUL00" } else { b" NULFF" });
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
