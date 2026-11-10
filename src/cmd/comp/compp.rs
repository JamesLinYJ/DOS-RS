//! Parser switches from `CMD/COMP/COMPP.ASM`.
//!
//! COMP calls the system parser with a near call and indexes parser data
//! from the data segment. File specifications, drive-only names, switches,
//! and file-table capitalization stay on. Dates, times, quoted strings,
//! numbers, keywords, and value lists stay off.

const NEAR_CALL: u8 = 0;
const FILE_SPEC: u8 = 1;
const FILE_CAPS: u8 = 1;
const DRIVE_ONLY: u8 = 1;
const SWITCHES: u8 = 1;
const USE_DATA_SEGMENT: u8 = 1;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMPP NR");
    put_hex8(NEAR_CALL);
    crate::bios::mscon::con_write_bytes(b" DT00 TM00 FI");
    put_hex8(FILE_SPEC);
    crate::bios::mscon::con_write_bytes(b" CA");
    put_hex8(FILE_CAPS);
    crate::bios::mscon::con_write_bytes(b" CX00 DV");
    put_hex8(DRIVE_ONLY);
    crate::bios::mscon::con_write_bytes(b" QU00 NU00 KY00 SW");
    put_hex8(SWITCHES);
    crate::bios::mscon::con_write_bytes(b" V100 V200 V300 IN00 BS");
    put_hex8(USE_DATA_SEGMENT);
    crate::bios::mscon::con_write_bytes(b" OK0000 ELFFFF MY0001 MS0002 NS0003 NK0004 RG0006 NV0007 ST0008 SY0009\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
