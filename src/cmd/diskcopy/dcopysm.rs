//! Message services from `CMD/DISKCOPY/DCOPYSM.ASM`.
//!
//! The utility name is DISKCOPY. Text comes from classes 1, 2, and A, in
//! that order. The linked services are load, display, character, number,
//! and input. A wrong DOS version is message 1, utility class, on the error
//! handle. A failure while loading messages uses class zero. A missing
//! message reports a length of zero.

const NAME: &[u8] = b"DISKCOPY";
const STDERR: u16 = 2;

struct Failure {
    number: u16,
    class: u8,
    handle: u16,
    count: u16,
    input: u8,
}

fn version_failure() -> Failure {
    Failure { number: 1, class: 0xFF, handle: STDERR, count: 0, input: 0 }
}

fn manager_failure() -> Failure {
    Failure { number: 1, class: 0, handle: STDERR, count: 0, input: 0 }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOPYSM NM");
    let mut index = 0usize;
    while index < NAME.len() {
        put_hex8(NAME[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" CL1 CL2 CLA LD DS CH NU IN");
    write_failure(&version_failure());
    write_failure(&manager_failure());
    crate::bios::mscon::con_write_bytes(b" NF01 CX0000\r\n");
}

fn write_failure(failure: &Failure) {
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex16(failure.number);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(failure.class);
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex16(failure.handle);
    crate::bios::mscon::con_write_bytes(b" K");
    put_hex16(failure.count);
    crate::bios::mscon::con_write_bytes(b" I");
    put_hex8(failure.input);
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
