//! Absolute disk read and write from `DOS/MSCODE.ASM`.
//!
//! The wrapper includes `MSSW.ASM`, so this is the IBM build. BIOS status
//! bytes are translated through `ERRIN`/`ERROUT` before they are returned.
//! Drive 0 is accepted. Adding one to drive 255 sets carry, so that unit is
//! rejected. Both the read and the write invalidate the secondary cache
//! because `IBMCOPYRIGHT` is off.

const IBM: u16 = 0xffff;
const BIOS_STATUS: [u8; 6] = [2, 6, 12, 4, 8, 0];
const ANCIENT_STATUS: [u8; 6] = [0x80, 0x40, 2, 0x10, 4, 3];

pub struct DiskCall {
    pub ancient: u8,
    pub failed: bool,
    pub cache_drive: u8,
}

/// Map one BIOS status through the IBM table. An unknown code stays 0.
pub fn map_bios(status: u8) -> u8 {
    let mut index = 0usize;
    while index < BIOS_STATUS.len() {
        if BIOS_STATUS[index] == status {
            return ANCIENT_STATUS[index];
        }
        index += 1;
    }
    0
}

/// `GETBP`. Carry is set for drive 255 and clear for a normal unit.
pub fn drive_ok(unit: u8) -> bool {
    unit != 0xff
}

/// `ABSDRD` / `ABSDWRT` after a BIOS status is known.
pub fn absolute(status: u8, unit: u8) -> DiskCall {
    if !drive_ok(unit) {
        return DiskCall {
            ancient: 0,
            failed: true,
            cache_drive: 0xff,
        };
    }
    DiskCall {
        ancient: map_bios(status),
        failed: true,
        cache_drive: 0xff,
    }
}

pub fn show() {
    let no_response = absolute(2, 0);
    let seek = absolute(6, 0);
    let good_drive = drive_ok(0);
    let bad_drive = drive_ok(0xff);
    let _failed = (no_response.failed, seek.failed);
    crate::bios::mscon::con_write_bytes(b"MSCODE IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" MAP02");
    put_hex8(no_response.ancient);
    crate::bios::mscon::con_write_bytes(b" MAP06");
    put_hex8(seek.ancient);
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex8(no_response.cache_drive);
    crate::bios::mscon::con_write_bytes(if good_drive { b" DRV00 OK" } else { b" DRV00 BAD" });
    crate::bios::mscon::con_write_bytes(if bad_drive { b" DRVFF OK" } else { b" DRVFF BAD" });
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
