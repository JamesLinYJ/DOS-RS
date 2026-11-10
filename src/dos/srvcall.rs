//! Server calls from `DOS/SRVCALL.ASM`.
//!
//! Functions 0 through 6 load the user and process ids. Functions 7, 8, and
//! 9 do not. Function 11 returns the swap table and does not dispatch.
//! Any other function loads the ids, then the table rejects an index of 11
//! or more with "invalid function" and an unknown locus. A server call marks
//! sharing as local. Commit skips an empty, busy, or remote file. The always
//! swap length keeps only its low 15 bits.

const TABLE_LIMIT: u8 = 11;
const ERROR_INVALID_FUNCTION: u16 = 1;
const LOCUS_UNKNOWN: u8 = 1;
const BUSY: u16 = 0xffff;
const SHARING_LOCAL: u8 = 0xff;
const MULTIPLEX_NET: u16 = 0x11;
const SPOOL_FUNCTION: u16 = 37;

#[derive(Clone, Copy)]
pub struct Packet {
    pub user_id: u16,
    pub process_id: u16,
}

#[derive(Clone, Copy)]
pub struct Ids {
    pub user_id: u16,
    pub process_id: u16,
}

pub enum Front {
    Table(Ids),
    Call { function: u8, ids: Ids },
}

pub struct OpenFile {
    pub references: u16,
    pub remote: bool,
}

pub struct Extended {
    pub error: u16,
    pub action: u8,
    pub class: u8,
    pub locus: u8,
    pub pointer: u16,
    pub pointer_segment: u16,
}

pub struct ServerCall {
    pub words: u8,
    pub skipped: u8,
    pub segments: u8,
    pub sharing: u8,
}

/// The `AL` checks before `TableDispatch`.
pub fn front(function: u8, packet: Packet, current: Ids) -> Front {
    if function < 7 {
        return Front::Call {
            function,
            ids: Ids {
                user_id: packet.user_id,
                process_id: packet.process_id,
            },
        };
    }
    if function <= 9 {
        return Front::Call {
            function,
            ids: current,
        };
    }
    if function == TABLE_LIMIT {
        return Front::Table(current);
    }
    Front::Call {
        function,
        ids: Ids {
            user_id: packet.user_id,
            process_id: packet.process_id,
        },
    }
}

/// `TableDispatch` rejects an index greater than or equal to the table byte.
pub fn dispatch(function: u8) -> Result<(), (u16, u8)> {
    if function >= TABLE_LIMIT {
        return Err((ERROR_INVALID_FUNCTION, LOCUS_UNKNOWN));
    }
    Ok(())
}

/// `SRV_CALL` copies six words, skips the saved frame, then copies two segments.
pub fn server_call() -> ServerCall {
    ServerCall {
        words: 6,
        skipped: 1,
        segments: 2,
        sharing: SHARING_LOCAL,
    }
}

/// `COMMIT_ALL`. Empty and busy files are skipped before the remote test.
pub fn commit_all(files: &[OpenFile]) -> u16 {
    let mut committed = 0u16;
    for file in files {
        if file.references == 0 {
            continue;
        }
        if file.references == BUSY {
            continue;
        }
        if file.remote {
            continue;
        }
        committed = committed.wrapping_add(1);
    }
    committed
}

/// `GET_DOS_DATA`. Only the always-swapped length loses its high bit.
pub fn dos_data(always_len: u16, swap_len: u16) -> (u16, u16) {
    (always_len & 0x7fff, swap_len)
}

/// `$SetExtendedError`. The locus is the high byte of the third word.
pub fn set_extended(
    error: u16,
    action_class: u16,
    locus_word: u16,
    pointer: u16,
    pointer_segment: u16,
) -> Extended {
    Extended {
        error,
        action: action_class as u8,
        class: (action_class >> 8) as u8,
        locus: (locus_word >> 8) as u8,
        pointer,
        pointer_segment,
    }
}

/// Close-by-name, close-by-user, close-by-user-and-process, and the open list.
pub fn share_slot(function: u8) -> Option<u8> {
    match function {
        2 => Some(5),
        3 => Some(3),
        4 => Some(4),
        5 => Some(9),
        _ => None,
    }
}

/// The default jump-table entries for those four calls are `BadCall`.
pub fn slot_accepts(function: u8) -> bool {
    share_slot(function).is_none()
}

/// Spool operations 7, 8, and 9. `AX` is multiplex `11h` and function 37.
pub fn spool_function() -> u16 {
    (MULTIPLEX_NET << 8) + SPOOL_FUNCTION
}

pub fn show() {
    let current = Ids {
        user_id: 0xabcd,
        process_id: 0x1111,
    };
    let data_call = front(
        6,
        Packet {
            user_id: 0x0006,
            process_id: 0,
        },
        current,
    );
    let spool = front(
        7,
        Packet {
            user_id: 0,
            process_id: 0,
        },
        current,
    );
    let last_spool = front(
        9,
        Packet {
            user_id: 0x1111,
            process_id: 0,
        },
        current,
    );
    let extended_call = front(
        10,
        Packet {
            user_id: 0x0042,
            process_id: 0x0043,
        },
        current,
    );
    let table = front(
        11,
        Packet {
            user_id: 0,
            process_id: 0,
        },
        current,
    );
    let invalid = front(
        12,
        Packet {
            user_id: 0x0007,
            process_id: 0x0008,
        },
        current,
    );
    let bad = match invalid {
        Front::Call { function, .. } => dispatch(function),
        Front::Table(_) => Err((0xffff, 0xff)),
    };
    let copied = server_call();
    let extended = set_extended(0x0020, 0x0504, 0x0700, 0x1234, 0x5678);
    let (always_len, swap_len) = dos_data(0x8003, 0x8010);
    let committed = commit_all(&[
        OpenFile {
            references: 0,
            remote: true,
        },
        OpenFile {
            references: BUSY,
            remote: false,
        },
        OpenFile {
            references: 2,
            remote: true,
        },
        OpenFile {
            references: 1,
            remote: false,
        },
    ]);
    let name = share_slot(2);
    let user = share_slot(3);
    let process = share_slot(4);
    let list = share_slot(5);
    let denied = !slot_accepts(2) && !slot_accepts(3) && !slot_accepts(4) && !slot_accepts(5);
    crate::bios::mscon::con_write_bytes(b"SRVCALL F6U");
    put_hex16(user_of(&data_call));
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex16(user_of(&spool));
    crate::bios::mscon::con_write_bytes(b" N9");
    put_hex16(user_of(&last_spool));
    crate::bios::mscon::con_write_bytes(b" F0A U");
    put_hex16(user_of(&extended_call));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(process_of(&extended_call));
    crate::bios::mscon::con_write_bytes(match table {
        Front::Table(_) => b" TB0B ID",
        Front::Call { .. } => b" TBFF ID",
    });
    put_hex16(user_of(&table));
    crate::bios::mscon::con_write_bytes(b" U");
    put_hex16(user_of(&invalid));
    match bad {
        Err((error, locus)) => {
            crate::bios::mscon::con_write_bytes(b" ER");
            put_hex16(error);
            crate::bios::mscon::con_write_bytes(b" LC");
            put_hex8(locus);
        }
        Ok(()) => crate::bios::mscon::con_write_bytes(b" ERFFFF LCFF"),
    }
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex8(copied.sharing);
    crate::bios::mscon::con_write_bytes(b" CPY");
    put_hex8(copied.words);
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(copied.skipped);
    crate::bios::mscon::con_write_bytes(b" SG");
    put_hex8(copied.segments);
    crate::bios::mscon::con_write_bytes(b" EE");
    put_hex16(extended.error);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(extended.action);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(extended.class);
    crate::bios::mscon::con_write_bytes(b" LO");
    put_hex8(extended.locus);
    crate::bios::mscon::con_write_bytes(b" PT");
    put_hex16(extended.pointer);
    crate::bios::mscon::con_write_bytes(b" ES");
    put_hex16(extended.pointer_segment);
    crate::bios::mscon::con_write_bytes(b" AD");
    put_hex16(always_len);
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex16(swap_len);
    crate::bios::mscon::con_write_bytes(b" CM");
    put_hex16(committed);
    crate::bios::mscon::con_write_bytes(b" NM");
    put_hex8(name.unwrap_or(0xff));
    crate::bios::mscon::con_write_bytes(b" CU");
    put_hex8(user.unwrap_or(0xff));
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex8(process.unwrap_or(0xff));
    crate::bios::mscon::con_write_bytes(b" GL");
    put_hex8(list.unwrap_or(0xff));
    crate::bios::mscon::con_write_bytes(if denied { b" NO SP" } else { b" OK SP" });
    put_hex16(spool_function());
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn user_of(front: &Front) -> u16 {
    match front {
        Front::Table(ids) | Front::Call { ids, .. } => ids.user_id,
    }
}

fn process_of(front: &Front) -> u16 {
    match front {
        Front::Table(ids) | Front::Call { ids, .. } => ids.process_id,
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
