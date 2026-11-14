//! 386 operand-size overrides with a real-mode, 16-bit address and stack size.
use super::{
    CpuState, GuestRam, Operand, Outcome, Segment, Slot, decode, fetch, pop_word, push_word,
};
fn imm(cpu: &mut CpuState, ram: &GuestRam) -> Option<u32> {
    Some(u32::from_le_bytes([
        fetch(cpu, ram)?,
        fetch(cpu, ram)?,
        fetch(cpu, ram)?,
        fetch(cpu, ram)?,
    ]))
}
fn read(cpu: &CpuState, ram: &GuestRam, op: &Operand) -> Option<u32> {
    match *op {
        Operand::Register(n) => Some(cpu.dword(Slot::from_index(n))),
        Operand::Memory { segment, offset } => {
            let mut b = [0; 4];
            for (n, v) in b.iter_mut().enumerate() {
                *v = ram.load(cpu.selector(segment), offset.checked_add(n as u16)?)?;
            }
            Some(u32::from_le_bytes(b))
        }
    }
}
fn write(cpu: &mut CpuState, ram: &mut GuestRam, op: &Operand, value: u32) -> Option<()> {
    match *op {
        Operand::Register(n) => cpu.set_dword(Slot::from_index(n), value),
        Operand::Memory { segment, offset } => {
            offset.checked_add(3)?;
            for (n, b) in value.to_le_bytes().iter().enumerate() {
                ram.store(cpu.selector(segment), offset + n as u16, *b);
            }
        }
    }
    Some(())
}
fn push(cpu: &mut CpuState, ram: &mut GuestRam, v: u32) {
    push_word(cpu, ram, (v >> 16) as u16);
    push_word(cpu, ram, v as u16);
}
fn pop(cpu: &mut CpuState, ram: &GuestRam) -> Option<u32> {
    Some(u32::from(pop_word(cpu, ram)?) | (u32::from(pop_word(cpu, ram)?) << 16))
}
fn alu(cpu: &mut CpuState, kind: u8, a: u32, b: u32) -> u32 {
    let carry = cpu.flags() & 1 != 0;
    let result = match kind {
        0 => a.wrapping_add(b),
        1 => a | b,
        2 => a.wrapping_add(b).wrapping_add(carry.into()),
        3 => a.wrapping_sub(b).wrapping_sub(carry.into()),
        4 => a & b,
        5 | 7 => a.wrapping_sub(b),
        _ => a ^ b,
    };
    let add = matches!(kind, 0 | 2);
    let sub = matches!(kind, 3 | 5 | 7);
    let cf = if add {
        u64::from(a) + u64::from(b) + u64::from(kind == 2 && carry) > u64::from(u32::MAX)
    } else {
        sub && u64::from(a) < u64::from(b) + u64::from(kind == 3 && carry)
    };
    let overflow = (if add {
        !(a ^ b) & (a ^ result)
    } else if sub {
        (a ^ b) & (a ^ result)
    } else {
        0
    }) >> 31
        != 0;
    let af = (add || sub) && (a ^ b ^ result) & 16 != 0;
    let flags = u16::from(cf)
        | (u16::from((result as u8).count_ones() % 2 == 0) << 2)
        | (u16::from(af) << 4)
        | (u16::from(result == 0) << 6)
        | (u16::from(result >> 31 != 0) << 7)
        | (u16::from(overflow) << 11);
    cpu.apply_result_flags(flags);
    result
}
fn shift(cpu: &mut CpuState, kind: u8, mut value: u32, count: u8) -> u32 {
    let count = count & 31;
    if count == 0 {
        return value;
    }
    let mut carry = cpu.flags() & 1 != 0;
    for _ in 0..count {
        let old = value;
        match kind {
            0 => {
                value = old.rotate_left(1);
                carry = old >> 31 != 0;
            }
            1 => {
                value = old.rotate_right(1);
                carry = old & 1 != 0;
            }
            2 => {
                value = old << 1 | u32::from(carry);
                carry = old >> 31 != 0;
            }
            3 => {
                value = old >> 1 | u32::from(carry) << 31;
                carry = old & 1 != 0;
            }
            4 | 6 => {
                value = old << 1;
                carry = old >> 31 != 0;
            }
            5 => {
                value = old >> 1;
                carry = old & 1 != 0;
            }
            _ => {
                value = ((old as i32) >> 1) as u32;
                carry = old & 1 != 0;
            }
        }
    }
    let mut flags = (cpu.flags() & !1) | u16::from(carry);
    if kind >= 4 {
        flags = (flags & !0xc4)
            | (u16::from((value as u8).count_ones().is_multiple_of(2)) << 2)
            | (u16::from(value == 0) << 6)
            | (u16::from(value >> 31 != 0) << 7);
    }
    if count == 1 {
        let overflow = match kind {
            0 | 2 | 4 | 6 => (value >> 31 != 0) ^ carry,
            1 | 3 | 5 => (value ^ (value << 1)) >> 31 != 0,
            _ => false,
        };
        flags = (flags & !0x800) | (u16::from(overflow) << 11);
    }
    cpu.set_flags(flags);
    value
}
pub(super) fn step(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    seg: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let result = (|| -> Option<()> {
        match opcode {
            0xc1 | 0xd1 | 0xd3 => {
                let m = fetch(cpu, ram)?;
                let op = decode(cpu, ram, m, seg)?;
                let n = match opcode {
                    0xc1 => fetch(cpu, ram)?,
                    0xd1 => 1,
                    _ => cpu.low(Slot::Count),
                };
                let v = read(cpu, ram, &op)?;
                let v = shift(cpu, (m >> 3) & 7, v, n);
                write(cpu, ram, &op, v)?;
            }
            0x50..=0x57 => push(cpu, ram, cpu.dword(Slot::from_index(opcode))),
            0x58..=0x5f => {
                let v = pop(cpu, ram)?;
                cpu.set_dword(Slot::from_index(opcode), v);
            }
            0x68 => {
                let v = imm(cpu, ram)?;
                push(cpu, ram, v);
            }
            0x6a => {
                let v = fetch(cpu, ram)? as i8 as i32 as u32;
                push(cpu, ram, v);
            }
            0x9c => push(cpu, ram, u32::from(cpu.flags())),
            0x9d => {
                let v = pop(cpu, ram)?;
                cpu.set_flags(v as u16);
            }
            0xb8..=0xbf => {
                let v = imm(cpu, ram)?;
                cpu.set_dword(Slot::from_index(opcode), v);
            }
            0xa1 | 0xa3 => {
                let offset = u16::from_le_bytes([fetch(cpu, ram)?, fetch(cpu, ram)?]);
                let op = Operand::Memory {
                    segment: seg.unwrap_or(Segment::Data),
                    offset,
                };
                if opcode == 0xa1 {
                    cpu.set_dword(Slot::Accumulator, read(cpu, ram, &op)?);
                } else {
                    write(cpu, ram, &op, cpu.dword(Slot::Accumulator))?;
                }
            }
            0x89 | 0x8b | 0xc7 | 0x81 | 0x83 | 0x85 | 0x01 | 0x03 | 0x09 | 0x0b | 0x11 | 0x13
            | 0x19 | 0x1b | 0x21 | 0x23 | 0x29 | 0x2b | 0x31 | 0x33 | 0x39 | 0x3b => {
                let m = fetch(cpu, ram)?;
                let r = (m >> 3) & 7;
                let reg = Operand::Register(r);
                let rm = decode(cpu, ram, m, seg)?;
                if opcode == 0xc7 {
                    if r != 0 {
                        return None;
                    }
                    let v = imm(cpu, ram)?;
                    return write(cpu, ram, &rm, v);
                }
                let (dest, src) = if opcode & 2 != 0 && !matches!(opcode, 0x81 | 0x83) {
                    (&reg, &rm)
                } else {
                    (&rm, &reg)
                };
                let value = match opcode {
                    0x81 => imm(cpu, ram)?,
                    0x83 => fetch(cpu, ram)? as i8 as i32 as u32,
                    _ => read(cpu, ram, src)?,
                };
                if matches!(opcode, 0x89 | 0x8b) {
                    return write(cpu, ram, dest, value);
                }
                let kind = match opcode {
                    0x81 | 0x83 => r,
                    0x85 => 4,
                    _ => opcode >> 3,
                };
                let res = alu(cpu, kind, read(cpu, ram, dest)?, value);
                if kind != 7 && opcode != 0x85 {
                    write(cpu, ram, dest, res)?;
                }
            }
            0x05 | 0x0d | 0x15 | 0x1d | 0x25 | 0x2d | 0x35 | 0x3d | 0xa9 => {
                let b = imm(cpu, ram)?;
                let kind = if opcode == 0xa9 { 4 } else { opcode >> 3 };
                let value = alu(cpu, kind, cpu.dword(Slot::Accumulator), b);
                if kind != 7 && opcode != 0xa9 {
                    cpu.set_dword(Slot::Accumulator, value);
                }
            }
            _ => return None,
        }
        Some(())
    })();
    if result.is_some() {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}
