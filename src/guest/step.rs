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
        0x80 => add_immediate(cpu, ram, override_segment, opcode),
        0xB0..=0xB7 => move_byte_immediate(cpu, ram, opcode),
        0xB8..=0xBF => move_word_immediate(cpu, ram, opcode),
        0xCD => software_interrupt(cpu, ram, opcode),
        0xF4 => Outcome::Halt,
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

fn read_byte(cpu: &CpuState, ram: &GuestRam, operand: &Operand) -> Option<u8> {
    match *operand {
        Operand::Register(index) => {
            let (slot, high) = byte_half(index);
            Some(if high { cpu.high(slot) } else { cpu.low(slot) })
        }
        Operand::Memory { segment, offset } => ram.load(cpu.selector(segment), offset),
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

fn fetch(cpu: &mut CpuState, ram: &GuestRam) -> Option<u8> {
    let byte = ram.load(cpu.selector(Segment::Code), cpu.instruction())?;
    cpu.set_instruction(cpu.instruction().wrapping_add(1));
    Some(byte)
}
