//! Device calls from `DOS/DEV.ASM`.
//!
//! `SETREAD` and `SETWRITE` fill the 22-byte request at `DEVCALL`.
//! `DEVIOCALL2` refuses a block read or write above 32 MB when the driver
//! is not extended: the status is done, error, and `error_I24_not_DOS_disk`.
//! `IOFUNC` on a regular file returns `1Ah` at end of file.

use super::handle;

const DEV_RD: u8 = 4;
const DEV_WRT: u8 = 8;
const DEV_WRT_VERIFY: u8 = 9;
const RD_WR_LEN: u8 = 22;
const ST_ERR: u16 = 0x8000;
const ST_DONE: u16 = 0x0100;
const NOT_DOS_DISK: u16 = 7;
const DEV_CHAR: u16 = 0x8000;
const EXTENDED: u16 = 0x0002;

/// Request header laid out like `SRHEAD` plus the read/write fields.
pub struct Call {
    pub bytes: [u8; 22],
}

/// `SETREAD`. `unit` is AL, `media` is AH, `count` is CX, `start` is DX.
pub fn set_read(unit: u8, media: u8, count: u16, start: u16) -> Call {
    set_call(DEV_RD, unit, media, count, start)
}

/// `SETWRITE`. `verify` is `VERFLG`: 0 writes, 1 writes and verifies.
pub fn set_write(unit: u8, media: u8, count: u16, start: u16, verify: u8) -> Call {
    set_call(DEV_WRT.wrapping_add(verify), unit, media, count, start)
}

fn set_call(command: u8, unit: u8, media: u8, count: u16, start: u16) -> Call {
    let mut bytes = [0u8; 22];
    bytes[0] = RD_WR_LEN;
    bytes[1] = unit;
    bytes[2] = command;
    bytes[13] = media;
    bytes[18] = count as u8;
    bytes[19] = (count >> 8) as u8;
    bytes[20] = start as u8;
    bytes[21] = (start >> 8) as u8;
    Call { bytes }
}

/// `DEVIOCALL2` for a block driver. A character device is not the >32 MB path.
///
/// `high_sector` is `HIGH_SECTOR`. `attr` is the device attribute word.
pub fn deviocall2(attr: u16, func: u8, high_sector: u16) -> u16 {
    let character = attr & DEV_CHAR != 0;
    if !character
        && (func == DEV_RD || func == DEV_WRT || func == DEV_WRT_VERIFY)
        && attr & EXTENDED == 0
        && high_sector != 0
    {
        return ST_ERR | ST_DONE | NOT_DOS_DISK;
    }
    ST_DONE
}

/// `IOFUNC` for a file. The bool is the zero flag: set means not ready or EOF.
pub fn iofunc_file(function: u8, handle_no: u16) -> (u8, bool) {
    if function == 0 {
        return io_in(handle_no);
    }
    let mut left = function.wrapping_sub(1);
    if left == 0 {
        return io_status(handle_no);
    }
    left = left.wrapping_sub(1);
    if left == 0 {
        return (0, false);
    }
    left = left.wrapping_sub(1);
    (0, left == 0)
}

fn io_in(handle_no: u16) -> (u8, bool) {
    let mut byte = [0u8; 1];
    let n = handle::read(handle_no, byte.as_mut_ptr(), 1);
    if n == 0 {
        (0x1a, true)
    } else {
        (byte[0], false)
    }
}

fn io_status(handle_no: u16) -> (u8, bool) {
    let pos = handle::seek(handle_no, 1, 0, 0).unwrap_or(0);
    let (result, zf) = io_in(handle_no);
    let _ = handle::seek(handle_no, 0, pos as u16, (pos >> 16) as u16);
    (result, zf)
}

pub fn show() {
    let read = set_read(0, 0xf8, 1, 7);
    let write = set_write(0, 0xf8, 1, 7, 0);
    let status = deviocall2(0, DEV_RD, 1);
    let name = b"BAR.COM\0";
    let handle_no = handle::open_asciiz(name.as_ptr()).unwrap_or(0xffff);
    let _ = handle::seek(handle_no, 2, 0, 0);
    let (eof, zf) = iofunc_file(0, handle_no);
    let _ = handle::close(handle_no);
    crate::bios::mscon::con_write_bytes(b"DEV HS");
    put_hex16(status);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(read.bytes[0]);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(read.bytes[2]);
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(write.bytes[2]);
    crate::bios::mscon::con_write_bytes(b" EOF");
    put_hex8(eof);
    crate::bios::mscon::con_write_bytes(if zf { b" Z\r\n" } else { b" NZ\r\n" });
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
