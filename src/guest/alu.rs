//! Eight-bit addition and the flags a later conditional would read.

const CARRY: u16 = 0x0001;
const PARITY: u16 = 0x0004;
const AUXILIARY: u16 = 0x0010;
const ZERO: u16 = 0x0040;
const SIGN: u16 = 0x0080;
const OVERFLOW: u16 = 0x0800;

pub fn add8(left: u8, right: u8) -> (u8, u16) {
    let sum = u16::from(left) + u16::from(right);
    let result = sum as u8;
    let mut flags = 0u16;
    if sum > 0xFF {
        flags |= CARRY;
    }
    if result == 0 {
        flags |= ZERO;
    }
    if result & 0x80 != 0 {
        flags |= SIGN;
    }
    if result.count_ones() % 2 == 0 {
        flags |= PARITY;
    }
    if (left ^ right ^ result) & 0x10 != 0 {
        flags |= AUXILIARY;
    }
    if (left ^ result) & (right ^ result) & 0x80 != 0 {
        flags |= OVERFLOW;
    }
    (result, flags)
}
