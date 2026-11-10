//! `DIV32`, `SHR32`, and the DOS 4 handle-full check from `DOS/DISK3.ASM`.
//!
//! `DIV32` divides a 32-bit sector count by the sector size. `SHR32` then
//! shifts that sector number by the cluster shift. A full disk with the
//! handle-fail bit set records end-of-file on a read and disk-full on a write.
//! `BUFF_RANGE_CHECK` matches the low sector, the high sector, and the drive.

use super::ms_code;

const EOF_ERROR: u16 = 38;
const FULL_ERROR: u16 = 39;
const CLASS_OUT_OF_RESOURCE: u8 = 1;
const ACTION_ABORT: u8 = 4;
const LOCUS_UNKNOWN: u8 = 1;
const HANDLE_FAIL: u16 = 0x0100;
const DISABLE_EOF: u16 = 0x0040;

/// `DIV32`. The quotient is `HIGH_SECTOR:AX` and DX is the remainder.
pub fn div32(dividend: u32, divisor: u16) -> (u32, u16) {
    let high = (dividend >> 16) as u16;
    let low = dividend as u16;
    let quot_high = high / divisor;
    let rem_high = high % divisor;
    let wide = ((rem_high as u32) << 16) | u32::from(low);
    let quot_low = (wide / u32::from(divisor)) as u16;
    let rem = (wide % u32::from(divisor)) as u16;
    (((quot_high as u32) << 16) | u32::from(quot_low), rem)
}

/// `SHR32`. `shift` is `dpb_cluster_shift`. Each step clears carry, then rotates.
pub fn shr32(high: u16, ax: u16, shift: u16) -> (u16, u16) {
    let mut value = ((high as u32) << 16) | u32::from(ax);
    let mut left = shift;
    while left != 0 {
        value >>= 1;
        left -= 1;
    }
    ((value >> 16) as u16, value as u16)
}

/// `File_Handle_Fail_Error`. `Ok` is carry clear. `Err` is the extended error.
pub fn file_handle_fail(disk_full: bool, dos34: u16, flags: u16, ah: u8) -> Result<(), u16> {
    if !disk_full || dos34 & DISABLE_EOF != 0 || flags & HANDLE_FAIL == 0 {
        return Ok(());
    }
    let code = if ah & 1 == 0 { EOF_ERROR } else { FULL_ERROR };
    ms_code::store(code, CLASS_OUT_OF_RESOURCE, ACTION_ABORT, LOCUS_UNKNOWN);
    Err(code)
}

/// `BUFF_RANGE_CHECK`. True means the sector is in this buffer.
pub fn buff_range(buf_sector: u32, buf_id: u8, drive: u8, sector: u32) -> bool {
    if buf_sector as u16 != sector as u16 {
        return false;
    }
    if (buf_sector >> 16) as u16 != (sector >> 16) as u16 {
        return false;
    }
    buf_id == drive
}

pub fn show() {
    let (quot, rem) = div32(0x0800_0000, 512);
    let (_high, cluster) = shr32(0, 0x0010, 3);
    let eof = file_handle_fail(true, 0, HANDLE_FAIL, 0);
    let (ax, bx, cx) = ms_code::get();
    let full = file_handle_fail(true, 0, HANDLE_FAIL, 1);
    let hit = buff_range(7, 0, 0, 7);
    let miss = buff_range(7, 0, 0, 8);
    crate::bios::mscon::con_write_bytes(b"DISK3 DIV");
    put_hex16((quot >> 16) as u16);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(quot as u16);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(rem);
    crate::bios::mscon::con_write_bytes(b" SHR");
    put_hex16(cluster);
    crate::bios::mscon::con_write_bytes(b" EOF");
    put_hex16(match eof {
        Ok(()) => 0,
        Err(code) => code,
    });
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((bx >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(bx as u8);
    crate::bios::mscon::con_write_bytes(b" FULL");
    put_hex16(match full {
        Ok(()) => 0,
        Err(code) => code,
    });
    let _ = (ax, cx);
    crate::bios::mscon::con_write_bytes(if hit && !miss { b" HIT\r\n" } else { b" MISS\r\n" });
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
