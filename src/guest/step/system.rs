//! 80386 system instructions used by 16-bit DOS extenders.
use super::super::pc386::Table;
use super::*;

pub(super) fn extended(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    seg: Option<Segment>,
    wide: bool,
) -> Outcome {
    match execute(cpu, ram, seg, wide) {
        Some(()) => Outcome::Continue,
        None => Outcome::Fault(0x0f),
    }
}
fn execute(cpu: &mut CpuState, ram: &mut GuestRam, seg: Option<Segment>, wide: bool) -> Option<()> {
    let op = fetch(cpu, ram)?;
    if matches!(op, 0x0b | 0xa6 | 0xa7 | 0xff) {
        cpu.machine.exception.set(Some((6, None)));
        return None;
    }
    if (0x80..=0x8f).contains(&op) {
        let disp = if wide {
            operand32::imm(cpu, ram)? as i32
        } else {
            i32::from(i16::from_le_bytes([fetch(cpu, ram)?, fetch(cpu, ram)?]))
        };
        if condition(cpu.flags(), op & 15) {
            cpu.advance_ip(disp);
        }
        return Some(());
    }
    if matches!(op, 0xa0 | 0xa1 | 0xa8 | 0xa9) {
        let s = if op < 0xa8 { Segment::Fs } else { Segment::Gs };
        if op & 1 == 0 {
            return push_stack(
                cpu,
                ram,
                u32::from(cpu.selector(s)),
                if wide { 4 } else { 2 },
            );
        }
        let sel = pop_stack(cpu, ram, if wide { 4 } else { 2 })? as u16;
        return load_segment(cpu, ram, s, sel).then_some(());
    }
    let privileged = !cpu.machine.protected() || cpu.machine.cpl == 0;
    if op == 6 {
        if !privileged {
            cpu.machine.exception.set(Some((13, Some(0))));
            return None;
        }
        cpu.machine.cr0 &= !8;
        return Some(());
    }
    let m = fetch(cpu, ram)?;
    let n = (m >> 3) & 7;
    if !privileged
        && (matches!(op, 0x20..=0x23)
            || op == 1 && matches!(n, 2 | 3 | 6)
            || op == 0 && matches!(n, 2 | 3))
    {
        cpu.machine.exception.set(Some((13, Some(0))));
        return None;
    }
    let reg = Slot::from_index(n);
    let rm = decode(cpu, ram, m, seg)?;
    let visible = |ram: &mut GuestRam, sel: u16| {
        let d = cpu.machine.descriptor(ram, sel)?;
        let requester = (sel & 3).max(cpu.selector(Segment::Code) & 3) as u8;
        ((d.access & 0x1c) == 0x1c || requester <= (d.access >> 5 & 3)).then_some(d)
    };
    match op {
        0x21 | 0x23 if m >> 6 == 3 => {
            let i = usize::from(if n == 4 || n == 5 { n + 2 } else { n });
            let r = Slot::from_index(m & 7);
            if op == 0x21 {
                cpu.set_dword(r, cpu.machine.debug[i]);
            } else {
                cpu.machine.debug[i] = cpu.dword(r);
            }
        }
        0xbc | 0xbd => {
            let val = if wide {
                operand32::read(cpu, ram, &rm)?
            } else {
                u32::from(read_word(cpu, ram, &rm)?)
            };
            cpu.set_zero(val == 0);
            if val != 0 {
                let n = if op == 0xbc {
                    val.trailing_zeros()
                } else {
                    31 - val.leading_zeros()
                };
                if wide {
                    cpu.set_dword(reg, n)
                } else {
                    cpu.set_wide(reg, n as u16)
                }
            }
        }
        0xaf => {
            if wide {
                let (v, ov) =
                    (cpu.dword(reg) as i32).overflowing_mul(operand32::read(cpu, ram, &rm)? as i32);
                cpu.set_dword(reg, v as u32);
                note_multiply_overflow(cpu, ov);
            } else {
                let (v, ov) =
                    (cpu.wide(reg) as i16).overflowing_mul(read_word(cpu, ram, &rm)? as i16);
                cpu.set_wide(reg, v as u16);
                note_multiply_overflow(cpu, ov);
            }
        }
        0xb2 | 0xb4 | 0xb5 => {
            let Operand::Memory { segment, offset } = rm else {
                return None;
            };
            let data = if wide {
                operand32::read(cpu, ram, &rm)?
            } else {
                u32::from(read_word(cpu, ram, &rm)?)
            };
            let sel = load_pair(
                cpu,
                ram,
                segment,
                offset.wrapping_add(if wide { 4 } else { 2 }),
            )?;
            let dest = match op {
                0xb2 => Segment::Stack,
                0xb4 => Segment::Fs,
                _ => Segment::Gs,
            };
            if !load_segment(cpu, ram, dest, sel) {
                return None;
            }
            if wide {
                cpu.set_dword(reg, data)
            } else {
                cpu.set_wide(reg, data as u16)
            }
        }
        0x20 | 0x22 if m >> 6 == 3 => {
            let g = Slot::from_index(m & 7);
            if op == 0x20 {
                let value = match n {
                    0 => cpu.machine.cr0,
                    2 => cpu.machine.cr2.get(),
                    3 => cpu.machine.cr3,
                    _ => return None,
                };
                cpu.set_dword(g, value)
            } else {
                let value = cpu.dword(g);
                match n {
                    0 if value >> 31 == 0 || value & 1 != 0 => cpu.machine.cr0 = value,
                    0 => {
                        cpu.machine.exception.set(Some((13, Some(0))));
                        return None;
                    }
                    2 => cpu.machine.cr2.set(value),
                    3 => cpu.machine.cr3 = value,
                    _ => return None,
                }
            }
        }
        1 if matches!(n, 0..=3) => {
            let Operand::Memory { segment, offset } = rm else {
                return None;
            };
            if n >= 2 {
                let mut b = [0; 6];
                for (i, v) in b.iter_mut().enumerate() {
                    *v = cpu.load_byte(ram, segment, offset.wrapping_add(i as u32))?;
                }
                let t = Table {
                    limit: u16::from_le_bytes([b[0], b[1]]),
                    base: u32::from_le_bytes([b[2], b[3], b[4], if wide { b[5] } else { 0 }]),
                };
                if n == 2 {
                    cpu.machine.gdtr = t
                } else {
                    cpu.machine.idtr = t
                }
            } else {
                let t = if n == 0 {
                    cpu.machine.gdtr
                } else {
                    cpu.machine.idtr
                };
                for (i, v) in t
                    .limit
                    .to_le_bytes()
                    .into_iter()
                    .chain(t.base.to_le_bytes())
                    .enumerate()
                {
                    if !cpu.store_byte(ram, segment, offset.wrapping_add(i as u32), v) {
                        return None;
                    }
                }
            }
        }
        1 if n == 4 => {
            if !write_word(cpu, ram, &rm, cpu.machine.cr0 as u16) {
                return None;
            }
        }
        1 if n == 6 => {
            let v = read_word(cpu, ram, &rm)?;
            cpu.machine.cr0 = (cpu.machine.cr0 & !14) | u32::from(v & 15);
        }
        0 => match n {
            0 | 1 => {
                let value = if n == 0 {
                    cpu.machine.ldt
                } else {
                    cpu.machine.tr
                };
                if !write_word(cpu, ram, &rm, value) {
                    return None;
                }
            }
            2 => cpu.machine.ldt = read_word(cpu, ram, &rm)?,
            3 => cpu.machine.tr = read_word(cpu, ram, &rm)?,
            4 | 5 => {
                let sel = read_word(cpu, ram, &rm)?;
                let d = visible(ram, sel);
                cpu.set_zero(d.is_some_and(|d| {
                    d.access & 0x90 == 0x90
                        && if n == 5 {
                            d.access & 10 == 2
                        } else {
                            d.access & 8 == 0 || d.access & 2 != 0
                        }
                }));
            }
            _ => return None,
        },
        2 | 3 => {
            let sel = read_word(cpu, ram, &rm)?;
            let d = visible(ram, sel).filter(|d| {
                d.access & 16 != 0
                    || matches!(d.access & 15, 1 | 2 | 3 | 9 | 11)
                    || op == 2 && matches!(d.access & 15, 4 | 5 | 12)
            });
            cpu.set_zero(d.is_some());
            if let Some(d) = d {
                let value = if op == 3 {
                    d.limit
                } else {
                    u32::from(d.access) << 8 | u32::from(d.flags & 0xf0) << 16
                };
                if wide {
                    cpu.set_dword(reg, value)
                } else {
                    cpu.set_wide(reg, value as u16)
                }
            }
        }
        0xa4 | 0xa5 | 0xac | 0xad => {
            let n = u32::from(if op & 1 == 0 {
                fetch(cpu, ram)?
            } else {
                cpu.low(Slot::Count)
            }) & 31;
            if n == 0 {
                return Some(());
            }
            let width = if wide { 32 } else { 16 };
            let mask = (1u64 << width) - 1;
            let dest = if wide {
                operand32::read(cpu, ram, &rm)?
            } else {
                u32::from(read_word(cpu, ram, &rm)?)
            };
            let src = u64::from(if wide {
                cpu.dword(reg)
            } else {
                u32::from(cpu.wide(reg))
            });
            let left = op & 8 == 0;
            let joined = if left {
                u64::from(dest) << width | src
            } else {
                src << width | u64::from(dest)
            };
            let v = if left {
                joined.wrapping_shl(n) >> width
            } else {
                joined >> n
            } & mask;
            let carry = if left {
                joined >> (2 * width - n)
            } else {
                joined >> (n - 1)
            } & 1;
            let sign = v >> (width - 1);
            let of = if n == 1 {
                ((sign ^ u64::from(dest >> (width - 1))) as u16) << 11
            } else {
                cpu.flags() & 0x800
            };
            cpu.apply_result_flags(
                carry as u16
                    | of
                    | (sign as u16) << 7
                    | u16::from(v == 0) << 6
                    | u16::from((v as u8).count_ones().is_multiple_of(2)) << 2,
            );
            if wide {
                operand32::write(cpu, ram, &rm, v as u32)?
            } else if !write_word(cpu, ram, &rm, v as u16) {
                return None;
            }
        }
        0xa3 | 0xab | 0xb3 | 0xbb | 0xba => {
            let bits = if wide { 32 } else { 16 };
            let (kind, idx) = if op == 0xba {
                if n < 4 {
                    return None;
                }
                (n - 4, i32::from(fetch(cpu, ram)?) % bits)
            } else {
                (
                    (op - 0xa3) / 8,
                    if wide {
                        cpu.dword(reg) as i32
                    } else {
                        i32::from(cpu.wide(reg) as i16)
                    },
                )
            };
            let rm = match rm {
                Operand::Memory { segment, offset } => Operand::Memory {
                    segment,
                    offset: offset.wrapping_add_signed(idx.div_euclid(bits) * (bits / 8)),
                },
                other => other,
            };
            let a = if wide {
                operand32::read(cpu, ram, &rm)?
            } else {
                u32::from(read_word(cpu, ram, &rm)?)
            };
            let mask = 1u32 << idx.rem_euclid(bits);
            cpu.set_carry(a & mask != 0);
            if kind != 0 {
                let v = match kind {
                    1 => a | mask,
                    2 => a & !mask,
                    _ => a ^ mask,
                };
                if wide {
                    operand32::write(cpu, ram, &rm, v)?
                } else if !write_word(cpu, ram, &rm, v as u16) {
                    return None;
                }
            }
        }
        0x90..=0x9f => {
            let v = u8::from(condition(cpu.flags(), op & 15));
            if !write_byte(cpu, ram, &rm, v) {
                return None;
            }
        }
        0xb6 | 0xbe | 0xb7 | 0xbf => {
            let value = match op {
                0xb6 => u32::from(read_byte(cpu, ram, &rm)?),
                0xbe => read_byte(cpu, ram, &rm)? as i8 as i32 as u32,
                0xb7 => u32::from(read_word(cpu, ram, &rm)?),
                _ => read_word(cpu, ram, &rm)? as i16 as i32 as u32,
            };
            if wide {
                cpu.set_dword(reg, value)
            } else {
                cpu.set_wide(reg, value as u16)
            }
        }
        _ => return None,
    }
    Some(())
}
