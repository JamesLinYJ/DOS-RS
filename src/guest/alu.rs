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

pub fn add16(left: u16, right: u16) -> (u16, u16) {
    let (low, low_flags) = add8(left as u8, right as u8);
    let carry = u16::from((low_flags & CARRY) != 0);
    let (high, high_flags) = add8((left >> 8) as u8, ((right >> 8) as u8).wrapping_add(carry as u8));
    let mut flags = high_flags & !CARRY & !AUXILIARY;
    if high_flags & CARRY != 0 {
        flags |= CARRY;
    }
    if low_flags & AUXILIARY != 0 {
        flags |= AUXILIARY;
    }
    if low == 0 && high == 0 {
        flags |= ZERO;
    } else {
        flags &= !ZERO;
    }
    (u16::from_le_bytes([low, high]), flags)
}

pub fn sub8(left: u8, right: u8) -> (u8, u16) {
    let result = left.wrapping_sub(right);
    let mut flags = 0u16;
    if left < right {
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
    if (left & 0x0F) < (right & 0x0F) {
        flags |= AUXILIARY;
    }
    if (left ^ right) & (left ^ result) & 0x80 != 0 {
        flags |= OVERFLOW;
    }
    (result, flags)
}

pub fn sub16(left: u16, right: u16) -> (u16, u16) {
    let (low, low_flags) = sub8(left as u8, right as u8);
    let borrow = u8::from((low_flags & CARRY) != 0);
    let (high, high_flags) = sub8((left >> 8) as u8, ((right >> 8) as u8).wrapping_add(borrow));
    let mut flags = high_flags & !CARRY & !AUXILIARY & !ZERO;
    if high_flags & CARRY != 0 {
        flags |= CARRY;
    }
    if low_flags & AUXILIARY != 0 {
        flags |= AUXILIARY;
    }
    if low == 0 && high == 0 {
        flags |= ZERO;
    }
    (u16::from_le_bytes([low, high]), flags)
}
