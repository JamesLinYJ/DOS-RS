//! Disk error handler from `CMD/DEBUG/DEBERR.ASM`.
//!
//! Japan mode is off, so the parity trap and release return without clearing
//! the flag or swapping interrupt 2. A drive marker of zero selects the
//! write-protect pair. Any other marker selects the drive pair. Inside
//! either pair the flag must equal the write function, 40h, or the read
//! message is used. The saved drive is then increased by 'A', and every
//! path resets the disk with function 0Dh.

const WRITE_FLAG: u8 = 64;
const READ_FLAG: u8 = 63;
const DISK_RESET: u8 = 13;

struct DiskError {
    letter: u8,
    message: u8,
}

fn disk_error(marker: u8, flag: u8, saved: u8) -> DiskError {
    let letter = saved.wrapping_add(b'A');
    let message = if marker != 0 {
        if flag == WRITE_FLAG { 2 } else { 1 }
    } else if flag == WRITE_FLAG {
        4
    } else {
        3
    };
    DiskError { letter, message }
}

fn trap_parity(flag: u8) -> u8 {
    flag
}

fn release_parity(flag: u8) -> u8 {
    flag
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBERR JP00 WR");
    put_hex8(WRITE_FLAG);
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(READ_FLAG);
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex8(DISK_RESET);
    write_error(1, WRITE_FLAG, 0);
    write_error(1, READ_FLAG, 1);
    write_error(0, WRITE_FLAG, 0);
    write_error(0, READ_FLAG, 2);
    write_error(1, 0, 0);
    write_error(1, WRITE_FLAG, 0xFF);
    let flag = 0u8;
    crate::bios::mscon::con_write_bytes(b" TP");
    put_hex8(trap_parity(flag));
    crate::bios::mscon::con_write_bytes(b" RL");
    put_hex8(release_parity(flag));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_error(marker: u8, flag: u8, saved: u8) {
    let error = disk_error(marker, flag, saved);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(error.letter);
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex8(error.message);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
