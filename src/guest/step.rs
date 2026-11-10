//! One guest instruction.
//!
//! An interrupt result means the pointer already addresses the next
//! instruction. The caller performs the service.

use super::alu;
use super::memory::GuestRam;
use super::state::{CpuState, Segment, Slot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Continue,
    Interrupt(u8),
    Halt,
    Fault(u8),
}

enum Operand {
    Register(u8),
    Memory { segment: Segment, offset: u16 },
}

pub fn step(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    let Some(mut opcode) = fetch(cpu, ram) else {
        return Outcome::Fault(0);
    };
    let mut override_segment = None;
    if matches!(opcode, 0x26 | 0x2E | 0x36 | 0x3E) {
        override_segment = Some(match opcode {
            0x26 => Segment::Extra,
            0x2E => Segment::Code,
            0x36 => Segment::Stack,
            _ => Segment::Data,
        });
        opcode = match fetch(cpu, ram) {
            Some(opcode) => opcode,
            None => return Outcome::Fault(0),
        };
    }
    match opcode {
        0x0F => machine_status(cpu, ram, opcode),
        0x3C => compare_byte(cpu, ram, opcode),
        0x3D => compare_word(cpu, ram, opcode),
        0x40..=0x47 => increment(cpu, opcode),
        0x60 => push_all(cpu, ram),
        0x61 => pop_all(cpu, ram),
        0x68 => push_immediate(cpu, ram, opcode),
        0x6A => push_signed(cpu, ram, opcode),
        0x70..=0x7F => branch(cpu, ram, opcode, false),
        0x80 => add_immediate(cpu, ram, override_segment, opcode),
        0x88 | 0x89 => move_into_operand(cpu, ram, override_segment, opcode),
        0x8A | 0x8B => move_from_operand(cpu, ram, override_segment, opcode),
        0x8E => move_into_segment(cpu, ram, override_segment, opcode),
        0xA0 => load_byte_absolute(cpu, ram, opcode),
        0xB0..=0xB7 => move_byte_immediate(cpu, ram, opcode),
        0xB8..=0xBF => move_word_immediate(cpu, ram, opcode),
        0xC0 => shift_byte(cpu, ram, override_segment, opcode),
        0xC3 => pop_instruction(cpu, ram),
        0xC6 => store_byte_immediate(cpu, ram, override_segment, opcode),
        0xCD => software_interrupt(cpu, ram, opcode),
        0xE8 => call_near(cpu, ram, opcode),
        0xEB => branch(cpu, ram, opcode, true),
        0xF4 => Outcome::Halt,
        0x50..=0x57 => push_register(cpu, ram, opcode),
        0x58..=0x5F => pop_register(cpu, ram, opcode),
        _ => Outcome::Fault(opcode),
    }
}

fn add_immediate(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Some(immediate) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 0 {
        return Outcome::Fault(opcode);
    }
    let Some(left) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let (result, flags) = alu::add8(left, immediate);
    if !write_byte(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    cpu.apply_result_flags(flags);
    Outcome::Continue
}

fn move_from_operand(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let wide = opcode & 1 == 1;
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let dest = (modrm >> 3) & 7;
    if wide {
        let Some(value) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.set_wide(Slot::from_index(dest), value);
    } else {
        let Some(value) = read_byte(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        let (slot, high) = byte_half(dest);
        if high {
            cpu.set_high(slot, value);
        } else {
            cpu.set_low(slot, value);
        }
    }
    Outcome::Continue
}

fn move_into_segment(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Some(value) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let Some(which) = Segment::from_index((modrm >> 3) & 7) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_selector(which, value);
    Outcome::Continue
}

fn move_byte_immediate(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(immediate) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let (slot, high) = byte_half(opcode & 7);
    if high {
        cpu.set_high(slot, immediate);
    } else {
        cpu.set_low(slot, immediate);
    }
    Outcome::Continue
}

fn move_word_immediate(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::from_index(opcode & 7), u16::from_le_bytes([low, high]));
    Outcome::Continue
}

fn machine_status(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(extended) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if extended != 0x01 {
        return Outcome::Fault(opcode);
    }
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 4 || modrm >> 6 != 3 {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::from_index(modrm & 7), 0);
    Outcome::Continue
}

fn compare_byte(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(immediate) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let (_, flags) = alu::sub8(cpu.low(Slot::Accumulator), immediate);
    cpu.apply_result_flags(flags);
    Outcome::Continue
}

fn compare_word(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let (_, flags) = alu::sub16(cpu.wide(Slot::Accumulator), u16::from_le_bytes([low, high]));
    cpu.apply_result_flags(flags);
    Outcome::Continue
}

fn increment(cpu: &mut CpuState, opcode: u8) -> Outcome {
    let slot = Slot::from_index(opcode & 7);
    let carry = cpu.flags() & 1;
    let (result, flags) = alu::add16(cpu.wide(slot), 1);
    cpu.set_wide(slot, result);
    cpu.apply_result_flags(flags);
    if carry != 0 {
        cpu.set_carry(true);
    }
    Outcome::Continue
}

fn push_all(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    let saved = cpu.wide(Slot::Stack);
    let order = [
        Slot::Accumulator,
        Slot::Count,
        Slot::Data,
        Slot::Base,
    ];
    for slot in order {
        if !push_word(cpu, ram, cpu.wide(slot)) {
            return Outcome::Fault(0x60);
        }
    }
    if !push_word(cpu, ram, saved) {
        return Outcome::Fault(0x60);
    }
    for slot in [Slot::Frame, Slot::Source, Slot::Dest] {
        if !push_word(cpu, ram, cpu.wide(slot)) {
            return Outcome::Fault(0x60);
        }
    }
    Outcome::Continue
}

fn pop_all(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    for slot in [Slot::Dest, Slot::Source, Slot::Frame] {
        let Some(value) = pop_word(cpu, ram) else {
            return Outcome::Fault(0x61);
        };
        cpu.set_wide(slot, value);
    }
    if pop_word(cpu, ram).is_none() {
        return Outcome::Fault(0x61);
    }
    for slot in [Slot::Base, Slot::Data, Slot::Count, Slot::Accumulator] {
        let Some(value) = pop_word(cpu, ram) else {
            return Outcome::Fault(0x61);
        };
        cpu.set_wide(slot, value);
    }
    Outcome::Continue
}

fn push_immediate(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if push_word(cpu, ram, u16::from_le_bytes([low, high])) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn push_signed(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(byte) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if push_word(cpu, ram, byte as i8 as u16) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn push_register(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let value = cpu.wide(Slot::from_index(opcode & 7));
    if push_word(cpu, ram, value) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn pop_register(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(value) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::from_index(opcode & 7), value);
    Outcome::Continue
}

fn branch(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8, always: bool) -> Outcome {
    let Some(byte) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if always || condition(cpu.flags(), opcode & 0x0F) {
        cpu.set_instruction(cpu.instruction().wrapping_add(byte as i8 as u16));
    }
    Outcome::Continue
}

fn call_near(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let next = cpu.instruction();
    if !push_word(cpu, ram, next) {
        return Outcome::Fault(opcode);
    }
    let disp = u16::from_le_bytes([low, high]);
    cpu.set_instruction(next.wrapping_add(disp));
    Outcome::Continue
}

fn pop_instruction(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(0xC3);
    };
    cpu.set_instruction(next);
    Outcome::Continue
}

fn load_byte_absolute(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(byte) = ram.load(cpu.selector(Segment::Data), u16::from_le_bytes([low, high])) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_low(Slot::Accumulator, byte);
    Outcome::Continue
}

fn store_byte_immediate(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Some(immediate) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 0 || !write_byte(cpu, ram, &operand, immediate) {
        return Outcome::Fault(opcode);
    }
    Outcome::Continue
}

fn shift_byte(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Some(count) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 5 {
        return Outcome::Fault(opcode);
    }
    let Some(mut value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let mut times = count;
    while times != 0 {
        value >>= 1;
        times -= 1;
    }
    if !write_byte(cpu, ram, &operand, value) {
        return Outcome::Fault(opcode);
    }
    Outcome::Continue
}

fn move_into_operand(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let wide = opcode & 1 == 1;
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let source = (modrm >> 3) & 7;
    if wide {
        let value = cpu.wide(Slot::from_index(source));
        if write_word(cpu, ram, &operand, value) {
            Outcome::Continue
        } else {
            Outcome::Fault(opcode)
        }
    } else {
        let (slot, high) = byte_half(source);
        let value = if high { cpu.high(slot) } else { cpu.low(slot) };
        if write_byte(cpu, ram, &operand, value) {
            Outcome::Continue
        } else {
            Outcome::Fault(opcode)
        }
    }
}

fn software_interrupt(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    match fetch(cpu, ram) {
        Some(vector) => Outcome::Interrupt(vector),
        None => Outcome::Fault(opcode),
    }
}

fn decode(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    modrm: u8,
    override_segment: Option<Segment>,
) -> Option<Operand> {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    if mode == 3 {
        return Some(Operand::Register(rm));
    }
    let (segment, offset) = if mode == 0 && rm == 6 {
        let low = fetch(cpu, ram)?;
        let high = fetch(cpu, ram)?;
        (Segment::Data, u16::from_le_bytes([low, high]))
    } else {
        let displacement = match mode {
            0 => 0,
            1 => {
                let byte = fetch(cpu, ram)?;
                byte as i8 as u16
            }
            _ => {
                let low = fetch(cpu, ram)?;
                let high = fetch(cpu, ram)?;
                u16::from_le_bytes([low, high])
            }
        };
        address(cpu, rm, displacement)
    };
    Some(Operand::Memory {
        segment: override_segment.unwrap_or(segment),
        offset,
    })
}

fn address(cpu: &CpuState, rm: u8, displacement: u16) -> (Segment, u16) {
    match rm & 7 {
        0 => (
            Segment::Data,
            cpu.wide(Slot::Base)
                .wrapping_add(cpu.wide(Slot::Source))
                .wrapping_add(displacement),
        ),
        1 => (
            Segment::Data,
            cpu.wide(Slot::Base)
                .wrapping_add(cpu.wide(Slot::Dest))
                .wrapping_add(displacement),
        ),
        2 => (
            Segment::Stack,
            cpu.wide(Slot::Frame)
                .wrapping_add(cpu.wide(Slot::Source))
                .wrapping_add(displacement),
        ),
        3 => (
            Segment::Stack,
            cpu.wide(Slot::Frame)
                .wrapping_add(cpu.wide(Slot::Dest))
                .wrapping_add(displacement),
        ),
        4 => (Segment::Data, cpu.wide(Slot::Source).wrapping_add(displacement)),
        5 => (Segment::Data, cpu.wide(Slot::Dest).wrapping_add(displacement)),
        6 => (Segment::Stack, cpu.wide(Slot::Frame).wrapping_add(displacement)),
        _ => (Segment::Data, cpu.wide(Slot::Base).wrapping_add(displacement)),
    }
}

fn read_word(cpu: &CpuState, ram: &GuestRam, operand: &Operand) -> Option<u16> {
    match *operand {
        Operand::Register(index) => Some(cpu.wide(Slot::from_index(index))),
        Operand::Memory { segment, offset } => {
            let low = ram.load(cpu.selector(segment), offset)?;
            let high = ram.load(cpu.selector(segment), offset.wrapping_add(1))?;
            Some(u16::from_le_bytes([low, high]))
        }
    }
}

fn read_byte(cpu: &CpuState, ram: &GuestRam, operand: &Operand) -> Option<u8> {
    match *operand {
        Operand::Register(index) => {
            let (slot, high) = byte_half(index);
            Some(if high { cpu.high(slot) } else { cpu.low(slot) })
        }
        Operand::Memory { segment, offset } => ram.load(cpu.selector(segment), offset),
    }
}

fn write_word(cpu: &mut CpuState, ram: &mut GuestRam, operand: &Operand, value: u16) -> bool {
    match *operand {
        Operand::Register(index) => {
            cpu.set_wide(Slot::from_index(index), value);
            true
        }
        Operand::Memory { segment, offset } => {
            let bytes = value.to_le_bytes();
            ram.store(cpu.selector(segment), offset, bytes[0])
                && ram.store(cpu.selector(segment), offset.wrapping_add(1), bytes[1])
        }
    }
}

fn write_byte(cpu: &mut CpuState, ram: &mut GuestRam, operand: &Operand, value: u8) -> bool {
    match *operand {
        Operand::Register(index) => {
            let (slot, high) = byte_half(index);
            if high {
                cpu.set_high(slot, value);
            } else {
                cpu.set_low(slot, value);
            }
            true
        }
        Operand::Memory { segment, offset } => ram.store(cpu.selector(segment), offset, value),
    }
}

fn byte_half(index: u8) -> (Slot, bool) {
    let high = index & 7 >= 4;
    (Slot::from_index(index & 3), high)
}

fn push_word(cpu: &mut CpuState, ram: &mut GuestRam, value: u16) -> bool {
    let pointer = cpu.wide(Slot::Stack).wrapping_sub(2);
    cpu.set_wide(Slot::Stack, pointer);
    let segment = cpu.selector(Segment::Stack);
    let bytes = value.to_le_bytes();
    ram.store(segment, pointer, bytes[0]) && ram.store(segment, pointer.wrapping_add(1), bytes[1])
}

fn pop_word(cpu: &mut CpuState, ram: &mut GuestRam) -> Option<u16> {
    let pointer = cpu.wide(Slot::Stack);
    let segment = cpu.selector(Segment::Stack);
    let low = ram.load(segment, pointer)?;
    let high = ram.load(segment, pointer.wrapping_add(1))?;
    cpu.set_wide(Slot::Stack, pointer.wrapping_add(2));
    Some(u16::from_le_bytes([low, high]))
}

fn condition(flags: u16, kind: u8) -> bool {
    let carry = flags & 1 != 0;
    let zero = flags & 0x40 != 0;
    let sign = flags & 0x80 != 0;
    let overflow = flags & 0x0800 != 0;
    match kind {
        0x2 => carry,
        0x3 => !carry,
        0x4 => zero,
        0x5 => !zero,
        0x6 => carry || zero,
        0x7 => !carry && !zero,
        0x8 => sign,
        0x9 => !sign,
        0xC => sign != overflow,
        0xD => sign == overflow,
        0xE => zero || sign != overflow,
        0xF => !zero && sign == overflow,
        _ => false,
    }
}

fn fetch(cpu: &mut CpuState, ram: &GuestRam) -> Option<u8> {
    let byte = ram.load(cpu.selector(Segment::Code), cpu.instruction())?;
    cpu.set_instruction(cpu.instruction().wrapping_add(1));
    Some(byte)
}
