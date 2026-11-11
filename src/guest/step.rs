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
        0x00..=0x05 | 0x08..=0x0D | 0x10..=0x15 | 0x18..=0x1D | 0x20..=0x25 | 0x28..=0x2D
        | 0x30..=0x35 | 0x38..=0x3D => arithmetic(cpu, ram, override_segment, opcode),
        0x37 => adjust_ascii_add(cpu),
        0x3F => adjust_ascii_sub(cpu),
        0x06 => push_segment(cpu, ram, Segment::Extra, opcode),
        0x07 => pop_segment(cpu, ram, Segment::Extra, opcode),
        0x0E => push_segment(cpu, ram, Segment::Code, opcode),
        0x0F => machine_status(cpu, ram, opcode),
        0x16 => push_segment(cpu, ram, Segment::Stack, opcode),
        0x17 => pop_segment(cpu, ram, Segment::Stack, opcode),
        0x1E => push_segment(cpu, ram, Segment::Data, opcode),
        0x1F => pop_segment(cpu, ram, Segment::Data, opcode),
        0x27 => decimal_add(cpu),
        0x2F => decimal_sub(cpu),
        0x40..=0x47 => increment(cpu, opcode),
        0x48..=0x4F => decrement(cpu, opcode),
        0x60 => push_all(cpu, ram),
        0x61 => pop_all(cpu, ram),
        0x68 => push_immediate(cpu, ram, opcode),
        0x69 => multiply_signed(cpu, ram, override_segment, opcode, true),
        0x6A => push_signed(cpu, ram, opcode),
        0x6B => multiply_signed(cpu, ram, override_segment, opcode, false),
        0x6C => input_string(cpu, ram, opcode),
        0x6E => output_string(cpu, ram, opcode),
        0x70..=0x7F => branch(cpu, ram, opcode, false),
        0x80 | 0x82 => add_immediate(cpu, ram, override_segment, opcode),
        0x84 | 0x85 => test_operands(cpu, ram, override_segment, opcode),
        0x81 | 0x83 => alter_word(cpu, ram, override_segment, opcode),
        0x88 | 0x89 => move_into_operand(cpu, ram, override_segment, opcode),
        0x8A | 0x8B => move_from_operand(cpu, ram, override_segment, opcode),
        0x8C => move_segment_out(cpu, ram, override_segment, opcode),
        0x8D => load_effective(cpu, ram, override_segment, opcode),
        0x8E => move_into_segment(cpu, ram, override_segment, opcode),
        0x8F => pop_operand(cpu, ram, override_segment, opcode),
        0x90 => Outcome::Continue,
        0x86 | 0x87 => exchange_operand(cpu, ram, override_segment, opcode),
        0x91..=0x97 => exchange(cpu, opcode),
        0x9A => call_far(cpu, ram, opcode),
        0x9B => Outcome::Continue,
        0xD8..=0xDF => float_skip(cpu, ram, override_segment, opcode),
        0x98 => extend_byte(cpu),
        0x99 => extend_word(cpu),
        0x9C => {
            if push_word(cpu, ram, cpu.flags()) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        0x9D => match pop_word(cpu, ram) {
            Some(value) => {
                cpu.set_flags(value);
                Outcome::Continue
            }
            None => Outcome::Fault(opcode),
        },
        0x9E => {
            let low = u16::from(cpu.high(Slot::Accumulator));
            cpu.set_flags((cpu.flags() & 0xFF00) | low);
            Outcome::Continue
        }
        0x9F => {
            cpu.set_high(Slot::Accumulator, cpu.flags() as u8);
            Outcome::Continue
        }
        0xA0 => load_byte_absolute(cpu, ram, override_segment, opcode),
        0xA1 => load_word_absolute(cpu, ram, override_segment, opcode),
        0xA2 => store_byte_absolute(cpu, ram, override_segment, opcode),
        0xA3 => store_word_absolute(cpu, ram, override_segment, opcode),
        0xA5 => {
            if copy_string_word(cpu, ram, override_segment) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        0xA6 => {
            if compare_string(cpu, ram, override_segment) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        0xA8 => test_byte(cpu, ram, opcode),
        0xA9 => test_word(cpu, ram, opcode),
        0xA4 => {
            if copy_string(cpu, ram, override_segment) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        0xAA => store_string(cpu, ram, opcode),
        0xAB => store_string_word(cpu, ram, opcode),
        0xAC => load_string(cpu, ram, override_segment, opcode),
        0xAD => load_string_word(cpu, ram, override_segment, opcode),
        0xAE => scan_string(cpu, ram, opcode),
        0xB0..=0xB7 => move_byte_immediate(cpu, ram, opcode),
        0xB8..=0xBF => move_word_immediate(cpu, ram, opcode),
        0xC0 => shift_byte(cpu, ram, override_segment, opcode),
        0xC1 => shift_word_immediate(cpu, ram, override_segment, opcode),
        0xC2 => return_count(cpu, ram, opcode),
        0xC3 => pop_instruction(cpu, ram),
        0xC4 => load_far(cpu, ram, override_segment, Segment::Extra, opcode),
        0xC5 => load_far(cpu, ram, override_segment, Segment::Data, opcode),
        0xC7 => store_word_immediate(cpu, ram, override_segment, opcode),
        0xCF => interrupt_return(cpu, ram, opcode),
        0xCA => return_far_count(cpu, ram, opcode),
        0xCB => return_far(cpu, ram, opcode),
        0xCC => Outcome::Interrupt(3),
        0xC6 => store_byte_immediate(cpu, ram, override_segment, opcode),
        0xC8 => enter(cpu, ram, opcode),
        0xC9 => leave(cpu, ram, opcode),
        0xCD => software_interrupt(cpu, ram, opcode),
        0xD0 => shift_once(cpu, ram, override_segment, opcode),
        0xD1 => shift_word_once(cpu, ram, override_segment, opcode),
        0xD2 => shift_by_count(cpu, ram, override_segment, false, opcode),
        0xD3 => shift_by_count(cpu, ram, override_segment, true, opcode),
        0xD4 => adjust_multiply(cpu, ram, opcode),
        0xD5 => adjust_divide(cpu, ram, opcode),
        0xD7 => translate(cpu, ram, opcode),
        0xE4 => read_port(cpu, ram, opcode),
        0xE0..=0xE2 => loop_count(cpu, ram, opcode),
        0xE3 => jump_count(cpu, ram, opcode),
        0xE8 => call_near(cpu, ram, opcode),
        0xE9 => jump_near(cpu, ram, opcode),
        0xEA => jump_far(cpu, ram, opcode),
        0xEB => branch(cpu, ram, opcode, true),
        0xF2 | 0xF3 => repeat(cpu, ram, override_segment, opcode),
        0xF4 => Outcome::Halt,
        0xF7 => unary_word(cpu, ram, override_segment, opcode),
        0xFA => {
            cpu.set_interrupt(false);
            Outcome::Continue
        }
        0xFB => {
            cpu.set_interrupt(true);
            Outcome::Continue
        }
        0xFE => adjust_byte(cpu, ram, override_segment, opcode),
        0xFF => adjust_word(cpu, ram, override_segment, opcode),
        0xF6 => multiply_byte(cpu, ram, override_segment, opcode),
        0xF5 => {
            cpu.set_carry((cpu.flags() & 1) == 0);
            Outcome::Continue
        }
        0xF8 => {
            cpu.set_carry(false);
            Outcome::Continue
        }
        0xF9 => {
            cpu.set_carry(true);
            Outcome::Continue
        }
        0xFC => {
            cpu.set_direction(false);
            Outcome::Continue
        }
        0xFD => {
            cpu.set_direction(true);
            Outcome::Continue
        }
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
    let Some(left) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let operation = (modrm >> 3) & 7;
    let carry = cpu.flags() & 1 != 0;
    let added = alu::add8(left, immediate);
    let with_carry = alu::adc8(left, immediate, carry);
    let borrowed = alu::sbb8(left, immediate, carry);
    let subtracted = alu::sub8(left, immediate);
    let (result, flags, store) = match operation {
        0 => (added.0, added.1, true),
        1 => (left | immediate, byte_logic(left | immediate), true),
        2 => (with_carry.0, with_carry.1, true),
        3 => (borrowed.0, borrowed.1, true),
        4 => (left & immediate, byte_logic(left & immediate), true),
        5 => (subtracted.0, subtracted.1, true),
        6 => (left ^ immediate, byte_logic(left ^ immediate), true),
        7 => (left, subtracted.1, false),
        _ => return Outcome::Fault(opcode),
    };
    if store && !write_byte(cpu, ram, &operand, result) {
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

fn float_skip(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if decode(cpu, ram, modrm, override_segment).is_none() {
        return Outcome::Fault(opcode);
    }
    Outcome::Continue
}

fn machine_status(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(extended) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if extended == 0x01 {
        return status_word(cpu, ram, opcode);
    }
    if extended == 0x00 {
        return local_table(cpu, ram, opcode);
    }
    Outcome::Fault(opcode)
}

fn status_word(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 4 || modrm >> 6 != 3 {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::from_index(modrm & 7), 0);
    Outcome::Continue
}

fn local_table(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 0 || modrm >> 6 != 3 {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::from_index(modrm & 7), 0);
    Outcome::Continue
}

fn decrement(cpu: &mut CpuState, opcode: u8) -> Outcome {
    let slot = Slot::from_index(opcode & 7);
    let carry = cpu.flags() & 1;
    let (result, flags) = alu::sub16(cpu.wide(slot), 1);
    cpu.set_wide(slot, result);
    cpu.apply_result_flags(flags);
    if carry != 0 {
        cpu.set_carry(true);
    }
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

fn call_far(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(entry_low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(entry_high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(code_low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(code_high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let next = cpu.instruction();
    let caller = cpu.selector(Segment::Code);
    if !push_word(cpu, ram, caller) || !push_word(cpu, ram, next) {
        return Outcome::Fault(opcode);
    }
    cpu.set_selector(Segment::Code, u16::from_le_bytes([code_low, code_high]));
    cpu.set_instruction(u16::from_le_bytes([entry_low, entry_high]));
    Outcome::Continue
}

fn jump_far(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(entry_low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(entry_high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(code_low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(code_high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_instruction(u16::from_le_bytes([entry_low, entry_high]));
    cpu.set_selector(Segment::Code, u16::from_le_bytes([code_low, code_high]));
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

fn load_byte_absolute(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let which = override_segment.unwrap_or(Segment::Data);
    let Some(byte) = ram.load(cpu.selector(which), u16::from_le_bytes([low, high])) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_low(Slot::Accumulator, byte);
    Outcome::Continue
}

fn load_word_absolute(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let which = override_segment.unwrap_or(Segment::Data);
    let offset = u16::from_le_bytes([low, high]);
    let Some(first) = ram.load(cpu.selector(which), offset) else {
        return Outcome::Fault(opcode);
    };
    let Some(second) = ram.load(cpu.selector(which), offset.wrapping_add(1)) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::Accumulator, u16::from_le_bytes([first, second]));
    Outcome::Continue
}

fn store_byte_absolute(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let which = override_segment.unwrap_or(Segment::Data);
    if ram.store(
        cpu.selector(which),
        u16::from_le_bytes([low, high]),
        cpu.low(Slot::Accumulator),
    ) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn store_word_absolute(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let which = override_segment.unwrap_or(Segment::Data);
    let offset = u16::from_le_bytes([low, high]);
    let bytes = cpu.wide(Slot::Accumulator).to_le_bytes();
    if ram.store(cpu.selector(which), offset, bytes[0])
        && ram.store(cpu.selector(which), offset.wrapping_add(1), bytes[1])
    {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
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
    let operation = (modrm >> 3) & 7;
    let Some(mut value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let original = u16::from(value);
    let mut carry = cpu.flags() & 1 != 0;
    let mut times = count & 0x1F;
    while times != 0 {
        let incoming = carry;
        match operation {
            0 => {
                carry = value & 0x80 != 0;
                value = (value << 1) | u8::from(carry);
            }
            1 => {
                carry = value & 1 != 0;
                value = (value >> 1) | (u8::from(carry) << 7);
            }
            2 => {
                carry = value & 0x80 != 0;
                value = (value << 1) | u8::from(incoming);
            }
            3 => {
                carry = value & 1 != 0;
                value = (value >> 1) | (u8::from(incoming) << 7);
            }
            4 | 6 => {
                carry = value & 0x80 != 0;
                value <<= 1;
            }
            7 => {
                carry = value & 1 != 0;
                value = ((value as i8) >> 1) as u8;
            }
            _ => {
                carry = value & 1 != 0;
                value >>= 1;
            }
        }
        times -= 1;
    }
    if !write_byte(cpu, ram, &operand, value) {
        return Outcome::Fault(opcode);
    }
    if count & 0x1F != 0 {
        cpu.apply_result_flags(sized_flags(u16::from(value), 0x0080));
    }
    cpu.set_carry(carry);
    if count & 0x1F == 1 {
        note_shift_overflow(cpu, operation, original, u16::from(value), 0x0080);
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

fn arithmetic(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let class = opcode >> 3;
    let form = opcode & 7;
    let wide = form & 1 == 1;
    let carry = cpu.flags() & 1 != 0;
    if form >= 4 {
        if wide {
            let Some(low) = fetch(cpu, ram) else {
                return Outcome::Fault(opcode);
            };
            let Some(high) = fetch(cpu, ram) else {
                return Outcome::Fault(opcode);
            };
            let right = u16::from_le_bytes([low, high]);
            let left = cpu.wide(Slot::Accumulator);
            let (result, flags, store) = combine_word(class, left, right, carry);
            if store {
                cpu.set_wide(Slot::Accumulator, result);
            }
            cpu.apply_result_flags(flags);
            return Outcome::Continue;
        }
        let Some(right) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let left = cpu.low(Slot::Accumulator);
        let (result, flags, store) = combine_byte(class, left, right, carry);
        if store {
            cpu.set_low(Slot::Accumulator, result);
        }
        cpu.apply_result_flags(flags);
        return Outcome::Continue;
    }
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let reg = (modrm >> 3) & 7;
    let dest_is_reg = form >= 2;
    if wide {
        let reg_value = cpu.wide(Slot::from_index(reg));
        let Some(mem_value) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        let (left, right) = if dest_is_reg {
            (reg_value, mem_value)
        } else {
            (mem_value, reg_value)
        };
        let (result, flags, store) = combine_word(class, left, right, carry);
        if store {
            if dest_is_reg {
                cpu.set_wide(Slot::from_index(reg), result);
            } else if !write_word(cpu, ram, &operand, result) {
                return Outcome::Fault(opcode);
            }
        }
        cpu.apply_result_flags(flags);
        return Outcome::Continue;
    }
    let (slot, high) = byte_half(reg);
    let reg_value = if high { cpu.high(slot) } else { cpu.low(slot) };
    let Some(mem_value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let (left, right) = if dest_is_reg {
        (reg_value, mem_value)
    } else {
        (mem_value, reg_value)
    };
    let (result, flags, store) = combine_byte(class, left, right, carry);
    if store {
        if dest_is_reg {
            if high {
                cpu.set_high(slot, result);
            } else {
                cpu.set_low(slot, result);
            }
        } else if !write_byte(cpu, ram, &operand, result) {
            return Outcome::Fault(opcode);
        }
    }
    cpu.apply_result_flags(flags);
    Outcome::Continue
}

fn combine_byte(class: u8, left: u8, right: u8, carry: bool) -> (u8, u16, bool) {
    match class {
        0 => {
            let (result, flags) = alu::add8(left, right);
            (result, flags, true)
        }
        1 => {
            let result = left | right;
            (result, byte_logic(result), true)
        }
        2 => {
            let (result, flags) = alu::adc8(left, right, carry);
            (result, flags, true)
        }
        3 => {
            let (result, flags) = alu::sbb8(left, right, carry);
            (result, flags, true)
        }
        4 => {
            let result = left & right;
            (result, byte_logic(result), true)
        }
        5 => {
            let (result, flags) = alu::sub8(left, right);
            (result, flags, true)
        }
        6 => {
            let result = left ^ right;
            (result, byte_logic(result), true)
        }
        _ => {
            let (_, flags) = alu::sub8(left, right);
            (left, flags, false)
        }
    }
}

fn combine_word(class: u8, left: u16, right: u16, carry: bool) -> (u16, u16, bool) {
    match class {
        0 => {
            let (result, flags) = alu::add16(left, right);
            (result, flags, true)
        }
        1 => {
            let result = left | right;
            (result, logic_flags(result), true)
        }
        2 => {
            let (result, flags) = alu::adc16(left, right, carry);
            (result, flags, true)
        }
        3 => {
            let (result, flags) = alu::sbb16(left, right, carry);
            (result, flags, true)
        }
        4 => {
            let result = left & right;
            (result, logic_flags(result), true)
        }
        5 => {
            let (result, flags) = alu::sub16(left, right);
            (result, flags, true)
        }
        6 => {
            let result = left ^ right;
            (result, logic_flags(result), true)
        }
        _ => {
            let (_, flags) = alu::sub16(left, right);
            (left, flags, false)
        }
    }
}

fn move_segment_out(
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
    let Some(which) = Segment::from_index((modrm >> 3) & 7) else {
        return Outcome::Fault(opcode);
    };
    if !write_word(cpu, ram, &operand, cpu.selector(which)) {
        return Outcome::Fault(opcode);
    }
    Outcome::Continue
}

fn pop_operand(
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
    if (modrm >> 3) & 7 != 0 {
        return Outcome::Fault(opcode);
    }
    let Some(value) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if write_word(cpu, ram, &operand, value) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn push_segment(cpu: &mut CpuState, ram: &mut GuestRam, which: Segment, opcode: u8) -> Outcome {
    if push_word(cpu, ram, cpu.selector(which)) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn pop_segment(cpu: &mut CpuState, ram: &mut GuestRam, which: Segment, opcode: u8) -> Outcome {
    let Some(value) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_selector(which, value);
    Outcome::Continue
}

fn return_far_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if return_far(cpu, ram, opcode) != Outcome::Continue {
        return Outcome::Fault(opcode);
    }
    let extra = u16::from_le_bytes([low, high]);
    cpu.set_wide(Slot::Stack, cpu.wide(Slot::Stack).wrapping_add(extra));
    Outcome::Continue
}

fn return_far(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(selector) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_instruction(next);
    cpu.set_selector(Segment::Code, selector);
    Outcome::Continue
}

fn shift_word_once(
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
    let operation = (modrm >> 3) & 7;
    let Some(value) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let carry_in = cpu.flags() & 1 != 0;
    let (result, carried) = match operation {
        0 => (
            (value << 1) | u16::from(value & 0x8000 != 0),
            value & 0x8000 != 0,
        ),
        1 => ((value >> 1) | ((value & 1) << 15), value & 1 != 0),
        2 => ((value << 1) | u16::from(carry_in), value & 0x8000 != 0),
        3 => ((value >> 1) | (u16::from(carry_in) << 15), value & 1 != 0),
        4 => (value << 1, value & 0x8000 != 0),
        5 => (value >> 1, value & 1 != 0),
        7 => ((value as i16 >> 1) as u16, value & 1 != 0),
        _ => return Outcome::Fault(opcode),
    };
    if !write_word(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    cpu.apply_result_flags(sized_flags(result, 0x8000));
    cpu.set_carry(carried);
    note_shift_overflow(cpu, operation, value, result, 0x8000);
    Outcome::Continue
}

fn shift_word_immediate(
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
    let operation = (modrm >> 3) & 7;
    if !matches!(operation, 0 | 1 | 2 | 3 | 4 | 5 | 7) {
        return Outcome::Fault(opcode);
    }
    let Some(mut value) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let original = value;
    let mut carry = cpu.flags() & 1 != 0;
    let mut times = count & 0x1F;
    while times != 0 {
        let incoming = carry;
        let (next, carried) = match operation {
            0 => (
                (value << 1) | u16::from(value & 0x8000 != 0),
                value & 0x8000 != 0,
            ),
            1 => ((value >> 1) | ((value & 1) << 15), value & 1 != 0),
            2 => ((value << 1) | u16::from(incoming), value & 0x8000 != 0),
            3 => ((value >> 1) | (u16::from(incoming) << 15), value & 1 != 0),
            4 => (value << 1, value & 0x8000 != 0),
            5 => (value >> 1, value & 1 != 0),
            _ => ((value as i16 >> 1) as u16, value & 1 != 0),
        };
        value = next;
        carry = carried;
        times -= 1;
    }
    if !write_word(cpu, ram, &operand, value) {
        return Outcome::Fault(opcode);
    }
    if count & 0x1F != 0 {
        cpu.apply_result_flags(sized_flags(value, 0x8000));
    }
    cpu.set_carry(carry);
    if count & 0x1F == 1 {
        note_shift_overflow(cpu, operation, original, value, 0x8000);
    }
    Outcome::Continue
}

fn adjust_byte(
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
    let Some(value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let carry = cpu.flags() & 1;
    let operation = (modrm >> 3) & 7;
    let (result, flags) = match operation {
        0 => alu::add8(value, 1),
        1 => alu::sub8(value, 1),
        _ => return Outcome::Fault(opcode),
    };
    if !write_byte(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    cpu.apply_result_flags(flags);
    if carry != 0 {
        cpu.set_carry(true);
    }
    Outcome::Continue
}

fn adjust_word(
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
    let operation = (modrm >> 3) & 7;
    match operation {
        0 | 1 => {
            let Some(value) = read_word(cpu, ram, &operand) else {
                return Outcome::Fault(opcode);
            };
            let carry = cpu.flags() & 1;
            let (result, flags) = if operation == 0 {
                alu::add16(value, 1)
            } else {
                alu::sub16(value, 1)
            };
            if !write_word(cpu, ram, &operand, result) {
                return Outcome::Fault(opcode);
            }
            cpu.apply_result_flags(flags);
            if carry != 0 {
                cpu.set_carry(true);
            }
            Outcome::Continue
        }
        2 | 4 => {
            let Some(target) = read_word(cpu, ram, &operand) else {
                return Outcome::Fault(opcode);
            };
            if operation == 2 {
                let next = cpu.instruction();
                if !push_word(cpu, ram, next) {
                    return Outcome::Fault(opcode);
                }
            }
            cpu.set_instruction(target);
            Outcome::Continue
        }
        3 | 5 => {
            let Operand::Memory { segment, offset } = operand else {
                return Outcome::Fault(opcode);
            };
            let which = cpu.selector(segment);
            let Some(entry_low) = ram.load(which, offset) else {
                return Outcome::Fault(opcode);
            };
            let Some(entry_high) = ram.load(which, offset.wrapping_add(1)) else {
                return Outcome::Fault(opcode);
            };
            let Some(code_low) = ram.load(which, offset.wrapping_add(2)) else {
                return Outcome::Fault(opcode);
            };
            let Some(code_high) = ram.load(which, offset.wrapping_add(3)) else {
                return Outcome::Fault(opcode);
            };
            if operation == 3 {
                let next = cpu.instruction();
                let caller = cpu.selector(Segment::Code);
                if !push_word(cpu, ram, caller) || !push_word(cpu, ram, next) {
                    return Outcome::Fault(opcode);
                }
            }
            cpu.set_instruction(u16::from_le_bytes([entry_low, entry_high]));
            cpu.set_selector(Segment::Code, u16::from_le_bytes([code_low, code_high]));
            Outcome::Continue
        }
        6 => {
            let Some(value) = read_word(cpu, ram, &operand) else {
                return Outcome::Fault(opcode);
            };
            if push_word(cpu, ram, value) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        _ => Outcome::Fault(opcode),
    }
}

fn store_string_word(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    if store_word(cpu, ram) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn store_word(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    let dest = cpu.wide(Slot::Dest);
    let bytes = cpu.wide(Slot::Accumulator).to_le_bytes();
    let segment = cpu.selector(Segment::Extra);
    if !ram.store(segment, dest, bytes[0]) || !ram.store(segment, dest.wrapping_add(1), bytes[1]) {
        return false;
    }
    cpu.set_wide(Slot::Dest, step_width(cpu, dest, 2));
    true
}

fn byte_logic(result: u8) -> u16 {
    let wide = u16::from(result);
    let signed = if result & 0x80 == 0 {
        wide
    } else {
        wide | 0x8000
    };
    logic_flags(signed)
}

fn alter_word(
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
    let immediate = if opcode == 0x83 {
        let Some(byte) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        byte as i8 as u16
    } else {
        let Some(low) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let Some(high) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        u16::from_le_bytes([low, high])
    };
    let Some(left) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let operation = (modrm >> 3) & 7;
    let carry = cpu.flags() & 1 != 0;
    let added = alu::add16(left, immediate);
    let with_carry = alu::adc16(left, immediate, carry);
    let borrowed = alu::sbb16(left, immediate, carry);
    let subtracted = alu::sub16(left, immediate);
    let (result, flags, store) = match operation {
        0 => (added.0, added.1, true),
        1 => (left | immediate, logic_flags(left | immediate), true),
        2 => (with_carry.0, with_carry.1, true),
        3 => (borrowed.0, borrowed.1, true),
        4 => (left & immediate, logic_flags(left & immediate), true),
        5 => (subtracted.0, subtracted.1, true),
        6 => (left ^ immediate, logic_flags(left ^ immediate), true),
        7 => (left, subtracted.1, false),
        _ => return Outcome::Fault(opcode),
    };
    if store && !write_word(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    cpu.apply_result_flags(flags);
    Outcome::Continue
}

fn load_effective(
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
    let value = match operand {
        Operand::Memory { offset, .. } => offset,
        Operand::Register(_) => cpu.recent_offset(),
    };
    cpu.set_wide(Slot::from_index((modrm >> 3) & 7), value);
    Outcome::Continue
}

fn multiply_byte(
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
    let operation = (modrm >> 3) & 7;
    if operation == 0 {
        let Some(immediate) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let Some(value) = read_byte(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.apply_result_flags(byte_logic(value & immediate));
        return Outcome::Continue;
    }
    let Some(value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    match operation {
        2 => {
            if !write_byte(cpu, ram, &operand, !value) {
                return Outcome::Fault(opcode);
            }
            Outcome::Continue
        }
        3 => {
            let (result, flags) = alu::sub8(0, value);
            if !write_byte(cpu, ram, &operand, result) {
                return Outcome::Fault(opcode);
            }
            cpu.apply_result_flags(flags);
            Outcome::Continue
        }
        4 => {
            let product = u16::from(cpu.low(Slot::Accumulator)).wrapping_mul(u16::from(value));
            cpu.set_wide(Slot::Accumulator, product);
            note_multiply_overflow(cpu, product > 0xFF);
            Outcome::Continue
        }
        5 => {
            let product = i16::from(cpu.low(Slot::Accumulator) as i8)
                .wrapping_mul(i16::from(value as i8));
            cpu.set_wide(Slot::Accumulator, product as u16);
            note_multiply_overflow(cpu, product != i16::from(product as i8));
            Outcome::Continue
        }
        6 => {
            if value == 0 {
                return Outcome::Fault(opcode);
            }
            let dividend = cpu.wide(Slot::Accumulator);
            let quotient = dividend / u16::from(value);
            if quotient > 0xFF {
                return Outcome::Fault(opcode);
            }
            let remainder = (dividend % u16::from(value)) as u8;
            cpu.set_low(Slot::Accumulator, quotient as u8);
            cpu.set_high(Slot::Accumulator, remainder);
            Outcome::Continue
        }
        7 => {
            let divisor = value as i8;
            if divisor == 0 {
                return Outcome::Fault(opcode);
            }
            let dividend = cpu.wide(Slot::Accumulator) as i16;
            let quotient = dividend / i16::from(divisor);
            if quotient < i16::from(i8::MIN) || quotient > i16::from(i8::MAX) {
                return Outcome::Fault(opcode);
            }
            let remainder = (dividend % i16::from(divisor)) as i8;
            cpu.set_low(Slot::Accumulator, quotient as u8);
            cpu.set_high(Slot::Accumulator, remainder as u8);
            Outcome::Continue
        }
        _ => Outcome::Fault(opcode),
    }
}

fn multiply_signed(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
    wide_factor: bool,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let factor = if wide_factor {
        let Some(low) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let Some(high) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        i16::from_le_bytes([low, high])
    } else {
        let Some(byte) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        byte as i8 as i16
    };
    let Some(source) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let wide = i32::from(source as i16).wrapping_mul(i32::from(factor));
    cpu.set_wide(Slot::from_index((modrm >> 3) & 7), wide as u16);
    note_multiply_overflow(cpu, wide != i32::from(wide as i16));
    Outcome::Continue
}

fn exchange(cpu: &mut CpuState, opcode: u8) -> Outcome {
    let slot = Slot::from_index(opcode & 7);
    let held = cpu.wide(Slot::Accumulator);
    cpu.set_wide(Slot::Accumulator, cpu.wide(slot));
    cpu.set_wide(slot, held);
    Outcome::Continue
}

fn extend_byte(cpu: &mut CpuState) -> Outcome {
    let value = cpu.low(Slot::Accumulator) as i8 as u16;
    cpu.set_wide(Slot::Accumulator, value);
    Outcome::Continue
}

fn extend_word(cpu: &mut CpuState) -> Outcome {
    let signed = cpu.wide(Slot::Accumulator) as i16;
    let high = if signed < 0 { 0xFFFF } else { 0 };
    cpu.set_wide(Slot::Data, high);
    Outcome::Continue
}

fn enter(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(level) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if level != 0 || !push_word(cpu, ram, cpu.wide(Slot::Frame)) {
        return Outcome::Fault(opcode);
    }
    let pointer = cpu.wide(Slot::Stack);
    cpu.set_wide(Slot::Frame, pointer);
    let bytes = u16::from_le_bytes([low, high]);
    cpu.set_wide(Slot::Stack, pointer.wrapping_sub(bytes));
    Outcome::Continue
}

fn leave(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    cpu.set_wide(Slot::Stack, cpu.wide(Slot::Frame));
    let Some(frame) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::Frame, frame);
    Outcome::Continue
}

fn shift_once(
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
    let operation = (modrm >> 3) & 7;
    let Some(value) = read_byte(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    let carry_in = cpu.flags() & 1 != 0;
    let (result, carried) = match operation {
        0 => ((value << 1) | u8::from(value & 0x80 != 0), value & 0x80 != 0),
        1 => ((value >> 1) | ((value & 1) << 7), value & 1 != 0),
        2 => ((value << 1) | u8::from(carry_in), value & 0x80 != 0),
        3 => ((value >> 1) | (u8::from(carry_in) << 7), value & 1 != 0),
        4 | 6 => (value << 1, value & 0x80 != 0),
        5 => (value >> 1, value & 1 != 0),
        _ => (((value as i8) >> 1) as u8, value & 1 != 0),
    };
    if !write_byte(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    cpu.apply_result_flags(sized_flags(u16::from(result), 0x0080));
    cpu.set_carry(carried);
    note_shift_overflow(cpu, operation, u16::from(value), u16::from(result), 0x0080);
    Outcome::Continue
}

fn loop_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(byte) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let count = cpu.wide(Slot::Count).wrapping_sub(1);
    cpu.set_wide(Slot::Count, count);
    let zero = cpu.flags() & 0x40 != 0;
    let branch = match opcode {
        0xE0 => count != 0 && !zero,
        0xE1 => count != 0 && zero,
        _ => count != 0,
    };
    if branch {
        cpu.set_instruction(cpu.instruction().wrapping_add(byte as i8 as u16));
    }
    Outcome::Continue
}

fn jump_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(byte) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if cpu.wide(Slot::Count) == 0 {
        cpu.set_instruction(cpu.instruction().wrapping_add(byte as i8 as u16));
    }
    Outcome::Continue
}

fn jump_near(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let next = cpu.instruction();
    cpu.set_instruction(next.wrapping_add(u16::from_le_bytes([low, high])));
    Outcome::Continue
}

fn exchange_operand(
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
    let reg = (modrm >> 3) & 7;
    if wide {
        let held = cpu.wide(Slot::from_index(reg));
        let Some(other) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.set_wide(Slot::from_index(reg), other);
        if write_word(cpu, ram, &operand, held) {
            Outcome::Continue
        } else {
            Outcome::Fault(opcode)
        }
    } else {
        let (slot, high) = byte_half(reg);
        let held = if high { cpu.high(slot) } else { cpu.low(slot) };
        let Some(other) = read_byte(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        if high {
            cpu.set_high(slot, other);
        } else {
            cpu.set_low(slot, other);
        }
        if write_byte(cpu, ram, &operand, held) {
            Outcome::Continue
        } else {
            Outcome::Fault(opcode)
        }
    }
}

fn test_operands(
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
    let reg = (modrm >> 3) & 7;
    if wide {
        let Some(other) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.apply_result_flags(logic_flags(cpu.wide(Slot::from_index(reg)) & other));
    } else {
        let (slot, high) = byte_half(reg);
        let left = if high { cpu.high(slot) } else { cpu.low(slot) };
        let Some(other) = read_byte(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.apply_result_flags(byte_logic(left & other));
    }
    Outcome::Continue
}

fn test_byte(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(immediate) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.apply_result_flags(byte_logic(cpu.low(Slot::Accumulator) & immediate));
    Outcome::Continue
}

fn test_word(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let immediate = u16::from_le_bytes([low, high]);
    cpu.apply_result_flags(logic_flags(cpu.wide(Slot::Accumulator) & immediate));
    Outcome::Continue
}

fn return_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_instruction(next);
    let extra = u16::from_le_bytes([low, high]);
    cpu.set_wide(Slot::Stack, cpu.wide(Slot::Stack).wrapping_add(extra));
    Outcome::Continue
}

fn load_far(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    target: Segment,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Operand::Memory { segment, offset } = operand else {
        return Outcome::Fault(opcode);
    };
    let which = cpu.selector(segment);
    let Some(low) = ram.load(which, offset) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = ram.load(which, offset.wrapping_add(1)) else {
        return Outcome::Fault(opcode);
    };
    let Some(seg_low) = ram.load(which, offset.wrapping_add(2)) else {
        return Outcome::Fault(opcode);
    };
    let Some(seg_high) = ram.load(which, offset.wrapping_add(3)) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(
        Slot::from_index((modrm >> 3) & 7),
        u16::from_le_bytes([low, high]),
    );
    cpu.set_selector(target, u16::from_le_bytes([seg_low, seg_high]));
    Outcome::Continue
}

fn store_word_immediate(
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
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if (modrm >> 3) & 7 != 0 {
        return Outcome::Fault(opcode);
    }
    if write_word(cpu, ram, &operand, u16::from_le_bytes([low, high])) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn interrupt_return(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(selector) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(flags) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_instruction(next);
    cpu.set_selector(Segment::Code, selector);
    cpu.set_flags(flags);
    Outcome::Continue
}

fn shift_by_count(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    wide: bool,
    opcode: u8,
) -> Outcome {
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let operation = (modrm >> 3) & 7;
    if operation > 5 && operation != 7 {
        return Outcome::Fault(opcode);
    }
    let count = cpu.low(Slot::Count) & 0x1F;
    let mut times = count;
    let mut carry = cpu.flags() & 1 != 0;
    let (original, shifted, sign) = if wide {
        let Some(mut value) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        let original = value;
        while times != 0 {
            let incoming = carry;
            match operation {
                0 => {
                    carry = value & 0x8000 != 0;
                    value = (value << 1) | u16::from(carry);
                }
                1 => {
                    carry = value & 1 != 0;
                    value = (value >> 1) | (u16::from(carry) << 15);
                }
                2 => {
                    carry = value & 0x8000 != 0;
                    value = (value << 1) | u16::from(incoming);
                }
                3 => {
                    carry = value & 1 != 0;
                    value = (value >> 1) | (u16::from(incoming) << 15);
                }
                4 => {
                    carry = value & 0x8000 != 0;
                    value <<= 1;
                }
                7 => {
                    carry = value & 1 != 0;
                    value = (value as i16 >> 1) as u16;
                }
                _ => {
                    carry = value & 1 != 0;
                    value >>= 1;
                }
            }
            times -= 1;
        }
        if !write_word(cpu, ram, &operand, value) {
            return Outcome::Fault(opcode);
        }
        if count != 0 {
            cpu.apply_result_flags(sized_flags(value, 0x8000));
        }
        (original, value, 0x8000u16)
    } else {
        let Some(mut value) = read_byte(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        let original = u16::from(value);
        while times != 0 {
            let incoming = carry;
            match operation {
                0 => {
                    carry = value & 0x80 != 0;
                    value = (value << 1) | u8::from(carry);
                }
                1 => {
                    carry = value & 1 != 0;
                    value = (value >> 1) | (u8::from(carry) << 7);
                }
                2 => {
                    carry = value & 0x80 != 0;
                    value = (value << 1) | u8::from(incoming);
                }
                3 => {
                    carry = value & 1 != 0;
                    value = (value >> 1) | (u8::from(incoming) << 7);
                }
                4 | 6 => {
                    carry = value & 0x80 != 0;
                    value <<= 1;
                }
                7 => {
                    carry = value & 1 != 0;
                    value = ((value as i8) >> 1) as u8;
                }
                _ => {
                    carry = value & 1 != 0;
                    value >>= 1;
                }
            }
            times -= 1;
        }
        if !write_byte(cpu, ram, &operand, value) {
            return Outcome::Fault(opcode);
        }
        if count != 0 {
            cpu.apply_result_flags(sized_flags(u16::from(value), 0x0080));
        }
        (original, u16::from(value), 0x0080)
    };
    cpu.set_carry(carry);
    if count == 1 {
        note_shift_overflow(cpu, operation, original, shifted, sign);
    }
    Outcome::Continue
}

fn decimal_add(cpu: &mut CpuState) -> Outcome {
    let old = cpu.low(Slot::Accumulator);
    let old_carry = cpu.flags() & 1 != 0;
    let auxiliary = cpu.flags() & 0x0010 != 0;
    let mut value = old;
    let mut carry = false;
    let mut adjusted = false;
    if auxiliary || (old & 0x0F) > 9 {
        let sum = u16::from(value) + 6;
        value = sum as u8;
        carry = old_carry || sum > 0xFF;
        adjusted = true;
    }
    if old > 0x99 || old_carry {
        value = value.wrapping_add(0x60);
        carry = true;
    }
    cpu.set_low(Slot::Accumulator, value);
    cpu.apply_result_flags(byte_logic(value));
    cpu.set_carry(carry);
    if adjusted {
        cpu.set_flags(cpu.flags() | 0x0010);
    }
    Outcome::Continue
}

fn decimal_sub(cpu: &mut CpuState) -> Outcome {
    let old = cpu.low(Slot::Accumulator);
    let old_carry = cpu.flags() & 1 != 0;
    let auxiliary = cpu.flags() & 0x0010 != 0;
    let mut value = old;
    let mut carry = false;
    let mut adjusted = false;
    if auxiliary || (old & 0x0F) > 9 {
        let borrowed = value < 6;
        value = value.wrapping_sub(6);
        carry = old_carry || borrowed;
        adjusted = true;
    }
    if old > 0x99 || old_carry {
        value = value.wrapping_sub(0x60);
        carry = true;
    }
    cpu.set_low(Slot::Accumulator, value);
    cpu.apply_result_flags(byte_logic(value));
    cpu.set_carry(carry);
    if adjusted {
        cpu.set_flags(cpu.flags() | 0x0010);
    }
    Outcome::Continue
}

fn note_multiply_overflow(cpu: &mut CpuState, overflow: bool) {
    cpu.set_carry(overflow);
    let mut flags = cpu.flags();
    if overflow {
        flags |= 0x0800;
    } else {
        flags &= !0x0800;
    }
    cpu.set_flags(flags);
}

fn note_shift_overflow(cpu: &mut CpuState, operation: u8, original: u16, result: u16, sign: u16) {
    let carried = cpu.flags() & 1 != 0;
    let high = result & sign != 0;
    let overflow = match operation {
        0 | 2 | 4 | 6 => carried != high,
        1 | 3 => high != (result & (sign >> 1) != 0),
        5 => original & sign != 0,
        _ => false,
    };
    if overflow {
        cpu.set_flags(cpu.flags() | 0x0800);
    }
}

fn adjust_ascii_sub(cpu: &mut CpuState) -> Outcome {
    let low = cpu.low(Slot::Accumulator);
    let auxiliary = cpu.flags() & 0x0010 != 0;
    let adjust = auxiliary || (low & 0x0F) > 9;
    let mut value = low;
    if adjust {
        value = value.wrapping_sub(6);
        cpu.set_high(
            Slot::Accumulator,
            cpu.high(Slot::Accumulator).wrapping_sub(1),
        );
    }
    value &= 0x0F;
    cpu.set_low(Slot::Accumulator, value);
    cpu.apply_result_flags(byte_logic(value));
    cpu.set_carry(adjust);
    if adjust {
        cpu.set_flags(cpu.flags() | 0x0010);
    }
    Outcome::Continue
}

fn adjust_ascii_add(cpu: &mut CpuState) -> Outcome {
    let low = cpu.low(Slot::Accumulator);
    let auxiliary = cpu.flags() & 0x0010 != 0;
    let adjust = auxiliary || (low & 0x0F) > 9;
    let mut value = low;
    if adjust {
        value = value.wrapping_add(6);
        cpu.set_high(
            Slot::Accumulator,
            cpu.high(Slot::Accumulator).wrapping_add(1),
        );
    }
    value &= 0x0F;
    cpu.set_low(Slot::Accumulator, value);
    cpu.apply_result_flags(byte_logic(value));
    cpu.set_carry(adjust);
    if adjust {
        cpu.set_flags(cpu.flags() | 0x0010);
    }
    Outcome::Continue
}

fn adjust_divide(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(base) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let high = cpu.high(Slot::Accumulator);
    let low = cpu.low(Slot::Accumulator);
    let value = low.wrapping_add(high.wrapping_mul(base));
    cpu.set_high(Slot::Accumulator, 0);
    cpu.set_low(Slot::Accumulator, value);
    cpu.apply_result_flags(byte_logic(value));
    Outcome::Continue
}

fn adjust_multiply(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(base) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    if base == 0 {
        return Outcome::Fault(opcode);
    }
    let value = cpu.low(Slot::Accumulator);
    let quotient = value / base;
    let remainder = value % base;
    cpu.set_high(Slot::Accumulator, quotient);
    cpu.set_low(Slot::Accumulator, remainder);
    cpu.apply_result_flags(byte_logic(remainder));
    Outcome::Continue
}

fn translate(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let address = cpu
        .wide(Slot::Base)
        .wrapping_add(u16::from(cpu.low(Slot::Accumulator)));
    let Some(byte) = ram.load(cpu.selector(Segment::Data), address) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_low(Slot::Accumulator, byte);
    Outcome::Continue
}

fn read_port(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    if fetch(cpu, ram).is_none() {
        return Outcome::Fault(opcode);
    }
    cpu.set_low(Slot::Accumulator, 0xFF);
    Outcome::Continue
}

fn unary_word(
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
    let operation = (modrm >> 3) & 7;
    if operation == 0 {
        let Some(low) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let Some(high) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        let Some(value) = read_word(cpu, ram, &operand) else {
            return Outcome::Fault(opcode);
        };
        cpu.apply_result_flags(logic_flags(value & u16::from_le_bytes([low, high])));
        return Outcome::Continue;
    }
    let Some(value) = read_word(cpu, ram, &operand) else {
        return Outcome::Fault(opcode);
    };
    match operation {
        2 => {
            if write_word(cpu, ram, &operand, !value) {
                Outcome::Continue
            } else {
                Outcome::Fault(opcode)
            }
        }
        3 => {
            let (result, flags) = alu::sub16(0, value);
            if !write_word(cpu, ram, &operand, result) {
                return Outcome::Fault(opcode);
            }
            cpu.apply_result_flags(flags);
            Outcome::Continue
        }
        4 => {
            let product = u32::from(cpu.wide(Slot::Accumulator)).wrapping_mul(u32::from(value));
            cpu.set_wide(Slot::Accumulator, product as u16);
            cpu.set_wide(Slot::Data, (product >> 16) as u16);
            note_multiply_overflow(cpu, product > 0xFFFF);
            Outcome::Continue
        }
        5 => {
            let product = i32::from(cpu.wide(Slot::Accumulator) as i16)
                .wrapping_mul(i32::from(value as i16));
            cpu.set_wide(Slot::Accumulator, product as u16);
            cpu.set_wide(Slot::Data, (product >> 16) as u16);
            note_multiply_overflow(cpu, product != i32::from(product as i16));
            Outcome::Continue
        }
        6 => {
            if value == 0 {
                return Outcome::Fault(opcode);
            }
            let dividend = (u32::from(cpu.wide(Slot::Data)) << 16)
                | u32::from(cpu.wide(Slot::Accumulator));
            let quotient = dividend / u32::from(value);
            if quotient > 0xFFFF {
                return Outcome::Fault(opcode);
            }
            cpu.set_wide(Slot::Accumulator, quotient as u16);
            cpu.set_wide(Slot::Data, (dividend % u32::from(value)) as u16);
            Outcome::Continue
        }
        7 => {
            let divisor = value as i16;
            if divisor == 0 {
                return Outcome::Fault(opcode);
            }
            let dividend = ((u32::from(cpu.wide(Slot::Data)) << 16)
                | u32::from(cpu.wide(Slot::Accumulator))) as i32;
            let quotient = dividend / i32::from(divisor);
            if quotient < i32::from(i16::MIN) || quotient > i32::from(i16::MAX) {
                return Outcome::Fault(opcode);
            }
            cpu.set_wide(Slot::Accumulator, quotient as u16);
            cpu.set_wide(Slot::Data, (dividend % i32::from(divisor)) as u16);
            Outcome::Continue
        }
        _ => Outcome::Fault(opcode),
    }
}

fn source_selector(cpu: &CpuState, override_segment: Option<Segment>) -> u16 {
    cpu.selector(override_segment.unwrap_or(Segment::Data))
}

fn copy_string_word(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
) -> bool {
    let source = cpu.wide(Slot::Source);
    let dest = cpu.wide(Slot::Dest);
    let segment = source_selector(cpu, override_segment);
    let Some(low) = ram.load(segment, source) else {
        return false;
    };
    let Some(high) = ram.load(segment, source.wrapping_add(1)) else {
        return false;
    };
    if !ram.store(cpu.selector(Segment::Extra), dest, low)
        || !ram.store(cpu.selector(Segment::Extra), dest.wrapping_add(1), high)
    {
        return false;
    }
    cpu.set_wide(Slot::Source, step_width(cpu, source, 2));
    cpu.set_wide(Slot::Dest, step_width(cpu, dest, 2));
    true
}

fn compare_string(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
) -> bool {
    let source = cpu.wide(Slot::Source);
    let dest = cpu.wide(Slot::Dest);
    let Some(left) = ram.load(source_selector(cpu, override_segment), source) else {
        return false;
    };
    let Some(right) = ram.load(cpu.selector(Segment::Extra), dest) else {
        return false;
    };
    let (_, flags) = alu::sub8(left, right);
    cpu.apply_result_flags(flags);
    cpu.set_wide(Slot::Source, step_index(cpu, source));
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    true
}

fn compare_string_word(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
) -> bool {
    let source = cpu.wide(Slot::Source);
    let dest = cpu.wide(Slot::Dest);
    let Some(left) = load_pair(ram, source_selector(cpu, override_segment), source) else {
        return false;
    };
    let Some(right) = load_pair(ram, cpu.selector(Segment::Extra), dest) else {
        return false;
    };
    let (_, flags) = alu::sub16(left, right);
    cpu.apply_result_flags(flags);
    cpu.set_wide(Slot::Source, step_width(cpu, source, 2));
    cpu.set_wide(Slot::Dest, step_width(cpu, dest, 2));
    true
}

fn load_word(cpu: &mut CpuState, ram: &mut GuestRam, override_segment: Option<Segment>) -> bool {
    let source = cpu.wide(Slot::Source);
    let Some(value) = load_pair(ram, source_selector(cpu, override_segment), source) else {
        return false;
    };
    cpu.set_wide(Slot::Accumulator, value);
    cpu.set_wide(Slot::Source, step_width(cpu, source, 2));
    true
}

fn scan_word(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    let dest = cpu.wide(Slot::Dest);
    let Some(value) = load_pair(ram, cpu.selector(Segment::Extra), dest) else {
        return false;
    };
    let (_, flags) = alu::sub16(cpu.wide(Slot::Accumulator), value);
    cpu.apply_result_flags(flags);
    cpu.set_wide(Slot::Dest, step_width(cpu, dest, 2));
    true
}

fn load_pair(ram: &GuestRam, segment: u16, offset: u16) -> Option<u16> {
    let low = ram.load(segment, offset)?;
    let high = ram.load(segment, offset.wrapping_add(1))?;
    Some(u16::from_le_bytes([low, high]))
}

fn scan_once(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    let dest = cpu.wide(Slot::Dest);
    let Some(value) = ram.load(cpu.selector(Segment::Extra), dest) else {
        return false;
    };
    let (_, flags) = alu::sub8(cpu.low(Slot::Accumulator), value);
    cpu.apply_result_flags(flags);
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    true
}

fn load_string_word(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let source = cpu.wide(Slot::Source);
    let segment = source_selector(cpu, override_segment);
    let Some(low) = ram.load(segment, source) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = ram.load(segment, source.wrapping_add(1)) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::Accumulator, u16::from_le_bytes([low, high]));
    cpu.set_wide(Slot::Source, step_width(cpu, source, 2));
    Outcome::Continue
}

fn scan_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    if scan_once(cpu, ram) {
        Outcome::Continue
    } else {
        Outcome::Fault(opcode)
    }
}

fn step_width(cpu: &CpuState, value: u16, width: u16) -> u16 {
    if cpu.direction_down() {
        value.wrapping_sub(width)
    } else {
        value.wrapping_add(width)
    }
}

fn repeat(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let Some(next) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let stop_when_zero = opcode == 0xF2;
    let compare = matches!(next, 0xA6 | 0xA7 | 0xAE | 0xAF);
    while cpu.wide(Slot::Count) != 0 {
        let ok = match next {
            0xA4 => copy_string(cpu, ram, override_segment),
            0xA5 => copy_string_word(cpu, ram, override_segment),
            0xA6 => compare_string(cpu, ram, override_segment),
            0xA7 => compare_string_word(cpu, ram, override_segment),
            0xAA => {
                let dest = cpu.wide(Slot::Dest);
                ram.store(
                    cpu.selector(Segment::Extra),
                    dest,
                    cpu.low(Slot::Accumulator),
                ) && {
                    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
                    true
                }
            }
            0xAB => store_word(cpu, ram),
            0xAD => load_word(cpu, ram, override_segment),
            0xAE => scan_once(cpu, ram),
            0xAF => scan_word(cpu, ram),
            _ => return Outcome::Fault(opcode),
        };
        if !ok {
            return Outcome::Fault(opcode);
        }
        cpu.set_wide(Slot::Count, cpu.wide(Slot::Count).wrapping_sub(1));
        if compare {
            let zero = cpu.flags() & 0x40 != 0;
            if stop_when_zero == zero {
                break;
            }
        }
    }
    Outcome::Continue
}

fn copy_string(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
) -> bool {
    let source = cpu.wide(Slot::Source);
    let dest = cpu.wide(Slot::Dest);
    let Some(byte) = ram.load(source_selector(cpu, override_segment), source) else {
        return false;
    };
    if !ram.store(cpu.selector(Segment::Extra), dest, byte) {
        return false;
    }
    cpu.set_wide(Slot::Source, step_index(cpu, source));
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    true
}

fn store_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let dest = cpu.wide(Slot::Dest);
    if !ram.store(cpu.selector(Segment::Extra), dest, cpu.low(Slot::Accumulator)) {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    Outcome::Continue
}

fn load_string(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let source = cpu.wide(Slot::Source);
    let Some(byte) = ram.load(source_selector(cpu, override_segment), source) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_low(Slot::Accumulator, byte);
    cpu.set_wide(Slot::Source, step_index(cpu, source));
    Outcome::Continue
}

fn input_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let dest = cpu.wide(Slot::Dest);
    let sample = if cpu.wide(Slot::Data) == 0 { b'Z' } else { 0 };
    if !ram.store(cpu.selector(Segment::Extra), dest, sample) {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    Outcome::Continue
}

fn output_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let source = cpu.wide(Slot::Source);
    if ram.load(cpu.selector(Segment::Data), source).is_none() {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::Source, step_index(cpu, source));
    Outcome::Continue
}

fn step_index(cpu: &CpuState, value: u16) -> u16 {
    if cpu.direction_down() {
        value.wrapping_sub(1)
    } else {
        value.wrapping_add(1)
    }
}

fn sized_flags(result: u16, sign: u16) -> u16 {
    let mut flags = 0u16;
    if result == 0 {
        flags |= 0x0040;
    }
    if result & sign != 0 {
        flags |= 0x0080;
    }
    if (result as u8).count_ones().is_multiple_of(2) {
        flags |= 0x0004;
    }
    flags
}

fn logic_flags(result: u16) -> u16 {
    let mut flags = 0u16;
    if result == 0 {
        flags |= 0x0040;
    }
    if result & 0x8000 != 0 {
        flags |= 0x0080;
    }
    if (result as u8).count_ones().is_multiple_of(2) {
        flags |= 0x0004;
    }
    flags
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
        let direct = u16::from_le_bytes([low, high]);
        cpu.set_recent_offset(direct);
        (Segment::Data, direct)
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
        let placed = address(cpu, rm, displacement);
        cpu.set_recent_offset(placed.1);
        placed
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
    let parity = flags & 0x0004 != 0;
    match kind {
        0x0 => overflow,
        0x1 => !overflow,
        0x2 => carry,
        0x3 => !carry,
        0x4 => zero,
        0x5 => !zero,
        0x6 => carry || zero,
        0x7 => !carry && !zero,
        0x8 => sign,
        0x9 => !sign,
        0xA => parity,
        0xB => !parity,
        0xC => sign != overflow,
        0xD => sign == overflow,
        0xE => zero || sign != overflow,
        0xF => !zero && sign == overflow,
        _ => false,
    }
}

fn fetch(cpu: &mut CpuState, ram: &GuestRam) -> Option<u8> {
    let byte = ram.load_code(cpu.selector(Segment::Code), cpu.instruction())?;
    cpu.set_instruction(cpu.instruction().wrapping_add(1));
    Some(byte)
}
