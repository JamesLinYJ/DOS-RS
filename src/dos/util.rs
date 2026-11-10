//! Handle utilities from `DOS/UTIL.ASM`.
//!
//! A handle at or past the JFN length is invalid. A slot of `FFh` is unused.
//! System-file indexes walk each table and stop when the link is `FFFFh`.
//! The first free JFN is the lowest `FFh`. A free system-file entry has a
//! zero reference. A busy entry is reused only when the user and process match.

const INVALID_HANDLE: u8 = 6;
const TOO_MANY: u8 = 4;
const UNUSED: u8 = 0xff;
const BUSY: u16 = 0xffff;

pub struct FileSlot {
    pub references: u16,
    pub user: u16,
    pub process: u16,
}

/// `pJFNFromHandle`. The handle must be below the allocated length.
pub fn jfn_slot(length: u16, handle: u16) -> Result<u16, u8> {
    if handle >= length {
        return Err(INVALID_HANDLE);
    }
    Ok(handle)
}

/// `SFFromSFN`. `None` means the chain ended at `FFFFh`.
pub fn locate_sfn(tables: &[&[FileSlot]], mut index: u16) -> Option<usize> {
    for (table_index, table) in tables.iter().enumerate() {
        let count = table.len() as u16;
        if index < count {
            return Some(table_index);
        }
        index -= count;
    }
    None
}

/// `SFFromHandle`.
pub fn file_from_handle(
    slots: &[u8],
    tables: &[&[FileSlot]],
    handle: u16,
) -> Result<Option<usize>, u8> {
    let slot = jfn_slot(slots.len() as u16, handle)?;
    let index = slots[slot as usize];
    if index == UNUSED {
        return Err(INVALID_HANDLE);
    }
    Ok(locate_sfn(tables, u16::from(index)))
}

/// `JFNFree`.
pub fn free_handle(slots: &[u8]) -> Result<u16, u8> {
    let mut handle = 0u16;
    loop {
        if jfn_slot(slots.len() as u16, handle).is_err() {
            return Err(TOO_MANY);
        }
        if slots[handle as usize] == UNUSED {
            return Ok(handle);
        }
        handle += 1;
    }
}

/// `SFNFree`. Debug is off, so a zero reference is taken immediately.
pub fn claim_file(entries: &[FileSlot], user: u16, process: u16) -> Result<u16, u8> {
    for (index, entry) in entries.iter().enumerate() {
        if entry.references == 0 {
            return Ok(index as u16);
        }
        if entry.references != BUSY {
            continue;
        }
        if entry.user != user {
            continue;
        }
        if entry.process != process {
            continue;
        }
        return Ok(index as u16);
    }
    Err(TOO_MANY)
}

pub fn show() {
    let slots = [UNUSED, 0x00, 0x02];
    let files = [
        FileSlot {
            references: 1,
            user: 0,
            process: 0,
        },
        FileSlot {
            references: BUSY,
            user: 9,
            process: 8,
        },
        FileSlot {
            references: 0,
            user: 0,
            process: 0,
        },
    ];
    let owned = [FileSlot {
        references: BUSY,
        user: 1,
        process: 2,
    }];
    let foreign = [FileSlot {
        references: BUSY,
        user: 9,
        process: 8,
    }];
    let tables: [&[FileSlot]; 2] = [&files[..2], &files[2..]];
    let past = jfn_slot(slots.len() as u16, 3);
    let inside = jfn_slot(slots.len() as u16, 1);
    let unused = file_from_handle(&slots, &tables, 0);
    let first = file_from_handle(&slots, &tables, 1);
    let second = file_from_handle(&slots, &tables, 2);
    let missing = locate_sfn(&tables, 3);
    let free = free_handle(&[0x01, UNUSED]);
    let full = free_handle(&[0x01]);
    let claimed = claim_file(&files, 1, 2);
    let ours = claim_file(&owned, 1, 2);
    let theirs = claim_file(&foreign, 1, 2);
    crate::bios::mscon::con_write_bytes(b"UTIL H");
    put_hex8(code(past));
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex8(match inside {
        Ok(handle) => handle as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" B");
    put_hex8(match unused {
        Err(error) => error,
        Ok(_) => 0,
    });
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(table_of(first));
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(table_of(second));
    crate::bios::mscon::con_write_bytes(if missing.is_none() { b" NO JF" } else { b" HIT JF" });
    put_hex8(match free {
        Ok(handle) => handle as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" JF");
    put_hex8(match full {
        Ok(handle) => handle as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" SN");
    put_hex8(match claimed {
        Ok(index) => index as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" OW");
    put_hex8(match ours {
        Ok(index) => index as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" NB");
    put_hex8(match theirs {
        Ok(index) => index as u8,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn code(result: Result<u16, u8>) -> u8 {
    match result {
        Ok(value) => value as u8,
        Err(error) => error,
    }
}

fn table_of(result: Result<Option<usize>, u8>) -> u8 {
    match result {
        Ok(Some(table)) => table as u8,
        _ => 0xff,
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
