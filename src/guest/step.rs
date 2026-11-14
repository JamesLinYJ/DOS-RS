//! One guest instruction.
//!
//! An interrupt result means the pointer already addresses the next
//! instruction. The caller performs the service.

#[cfg_attr(test, path = "step/operand32.rs")]
mod operand32;
#[cfg(feature = "pc386")]
#[cfg_attr(test, path = "step/system.rs")]
mod system;
use super::alu;
use super::memory::GuestRam;
use super::state::{CpuState, Segment, Slot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Continue,
    Xms,
    Ems,
    EmsReturn,
    Interrupt(u8),
    #[cfg(feature = "pc386")]
    Firmware(u8),
    Port {
        port: u16,
        write: bool,
        word: bool,
    },
    Halt,
    Fault(u8),
}

enum Operand {
    Register(u8),
    Memory { segment: Segment, offset: u32 },
}

/// Run ordinary instructions until the next service or the scheduler's budget.
pub fn slice(cpu: &mut CpuState, ram: &mut GuestRam, budget: u32) -> (Outcome, u16) {
    #[cfg(feature = "pc386")]
    if ram.graphics.as_ref().is_some_and(|g| g.active)
        && let Some(device) = &mut ram.at
    {
        device.advance(budget.min(1024).div_ceil(8));
        if cpu.flags() & 0x200 != 0
            && !cpu.machine.inhibit
            && let Some(vector) = device.interrupt()
        {
            if super::pc386::interrupt(cpu, ram, vector).is_none() {
                return (Outcome::Fault(0xcd), cpu.instruction());
            }
        }
    }
    let mut at = cpu.instruction();
    for _ in 0..budget {
        at = cpu.instruction();
        #[cfg(feature = "pc386")]
        let sp = cpu.dword(Slot::Stack);
        let event = step(cpu, ram);
        #[cfg(feature = "pc386")]
        let event = if let Some((v, error)) = cpu.machine.exception.take() {
            cpu.set_instruction(at);
            cpu.set_dword(Slot::Stack, sp);
            if super::pc386::interrupt(cpu, ram, v).is_some()
                && error.is_none_or(|code| push_word(cpu, ram, code))
            {
                Outcome::Continue
            } else {
                Outcome::Fault(0xcd)
            }
        } else {
            event
        };

        #[cfg(feature = "pc386")]
        let event = match event {
            Outcome::Interrupt(v) if super::pc386::has_vector(cpu, ram, v) => {
                if super::pc386::interrupt(cpu, ram, v).is_some() {
                    Outcome::Continue
                } else {
                    Outcome::Fault(0xcd)
                }
            }
            other => other,
        };
        if event != Outcome::Continue {
            return (event, at);
        }
    }
    (Outcome::Continue, at)
}

#[cfg(feature = "pc386")]
pub(super) fn firmware(cs: u16, ip: u16) -> Option<u8> {
    (cs == 0xf000 && (0x200..0x400).contains(&ip) && ip & 1 == 0).then(|| ((ip - 0x200) / 2) as u8)
}
pub fn step(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    let start = cpu.instruction();
    #[cfg(feature = "pc386")]
    {
        cpu.machine.inhibit = false;
    }
    #[cfg(feature = "pc386")]
    let real = !cpu.machine.protected();
    #[cfg(not(feature = "pc386"))]
    let real = true;
    if real {
        #[cfg(feature = "pc386")]
        if let Some(v) = firmware(cpu.selector(Segment::Code), start) {
            return Outcome::Firmware(v);
        }
        if (cpu.selector(Segment::Code), start) == super::run::xms::CONTROL {
            return Outcome::Xms;
        }
        if (cpu.selector(Segment::Code), start) == super::run::ems::ENTRY {
            return Outcome::Ems;
        }
        if (cpu.selector(Segment::Code), start) == super::run::ems::RETURN {
            return Outcome::EmsReturn;
        }
    }
    let Some(mut opcode) = fetch(cpu, ram) else {
        return Outcome::Fault(0);
    };
    let mut override_segment = None;
    let mut dword = false;
    let mut address32 = false;
    let mut rep = None;
    loop {
        if matches!(opcode, 0x26 | 0x2E | 0x36 | 0x3E) {
            override_segment = Some(match opcode {
                0x26 => Segment::Extra,
                0x2E => Segment::Code,
                0x36 => Segment::Stack,
                _ => Segment::Data,
            });
        } else if matches!(opcode, 0x64 | 0x65) && cfg!(feature = "pc386") {
            override_segment = Segment::from_index(opcode - 0x60);
        } else if matches!(opcode, 0xf2 | 0xf3) {
            rep = Some(opcode);
        } else if opcode == 0x67 {
            address32 = true;
        } else if opcode == 0x66 {
            dword = true;
        } else if opcode != 0xF0 {
            break;
        }
        if cpu.instruction().wrapping_sub(start) >= 15 {
            return Outcome::Fault(opcode);
        }
        opcode = match fetch(cpu, ram) {
            Some(opcode) => opcode,
            None => return Outcome::Fault(0),
        };
    }
    cpu.address32 = address32;
    if matches!(opcode,0xa4..=0xa7|0xaa..=0xaf) {
        return string_instruction(cpu, ram, override_segment, opcode, dword, rep, start);
    }
    if rep.is_some() && opcode != 0x90 {
        return Outcome::Fault(opcode);
    }

    #[cfg(feature = "pc386")]
    if opcode == 0x0f {
        return system::extended(cpu, ram, override_segment, dword);
    }
    if address32 {
        if opcode == 0xe3 {
            let Some(disp) = fetch(cpu, ram) else {
                return Outcome::Fault(opcode);
            };
            if cpu.dword(Slot::Count) == 0 {
                cpu.set_instruction(cpu.instruction().wrapping_add_signed(i16::from(disp as i8)));
            }
            return Outcome::Continue;
        }
        #[cfg(not(feature = "pc386"))]
        return Outcome::Fault(opcode);
    }
    if (0xa0..=0xa3).contains(&opcode) {
        return if absolute_move(cpu, ram, override_segment, opcode, dword).is_some() {
            Outcome::Continue
        } else {
            Outcome::Fault(opcode)
        };
    }
    if dword {
        return operand32::step(cpu, ram, override_segment, opcode);
    }
    match opcode {
        0x00..=0x05
        | 0x08..=0x0D
        | 0x10..=0x15
        | 0x18..=0x1D
        | 0x20..=0x25
        | 0x28..=0x2D
        | 0x30..=0x35
        | 0x38..=0x3D => arithmetic(cpu, ram, override_segment, opcode),
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
        0x62 => bound_check(cpu, ram, override_segment, opcode),
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
        0xA8 => test_byte(cpu, ram, opcode),
        0xA9 => test_word(cpu, ram, opcode),
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
        0xCB => return_far(cpu, ram, opcode, 0),
        0xCC => Outcome::Interrupt(3),
        0xC6 => store_byte_immediate(cpu, ram, override_segment, opcode),
        0xC8 => enter(cpu, ram, opcode),
        0xC9 => leave(cpu, ram, opcode),
        0xCD => software_interrupt(cpu, ram, opcode),
        0xCE => interrupt_on_overflow(cpu),
        0xD0 => shift_once(cpu, ram, override_segment, opcode),
        0xD1 => shift_word_once(cpu, ram, override_segment, opcode),
        0xD2 => shift_by_count(cpu, ram, override_segment, false, opcode),
        0xD3 => shift_by_count(cpu, ram, override_segment, true, opcode),
        0xD4 => adjust_multiply(cpu, ram, opcode),
        0xD5 => adjust_divide(cpu, ram, opcode),
        0xD6 => set_byte_from_carry(cpu),
        0xD7 => translate(cpu, ram, override_segment, opcode),
        0xE4..=0xE7 | 0xEC..=0xEF => port_io(cpu, ram, opcode),
        0xE0..=0xE2 => loop_count(cpu, ram, opcode),
        0xE3 => jump_count(cpu, ram, opcode),
        0xE8 => call_near(cpu, ram, opcode),
        0xE9 => jump_near(cpu, ram, opcode),
        0xEA => jump_far(cpu, ram, opcode),
        0xEB => branch(cpu, ram, opcode, true),

        0xF4 => Outcome::Halt,
        0xF7 => unary_word(cpu, ram, override_segment, opcode),
        0xFA => {
            cpu.set_interrupt(false);
            Outcome::Continue
        }
        0xFB => {
            #[cfg(feature = "pc386")]
            {
                cpu.machine.inhibit = true;
            }
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
    if !load_segment(cpu, ram, which, value) {
        return Outcome::Fault(opcode);
    }
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
    cpu.set_wide(
        Slot::from_index(opcode & 7),
        u16::from_le_bytes([low, high]),
    );
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
    if (0x80..=0x8f).contains(&extended) {
        let (Some(lo), Some(hi)) = (fetch(cpu, ram), fetch(cpu, ram)) else {
            return Outcome::Fault(opcode);
        };
        if condition(cpu.flags(), extended & 15) {
            cpu.set_instruction(cpu.instruction().wrapping_add(u16::from_le_bytes([lo, hi])));
        }
        return Outcome::Continue;
    }
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
    cpu.set_carry(carry != 0);
    Outcome::Continue
}

fn increment(cpu: &mut CpuState, opcode: u8) -> Outcome {
    let slot = Slot::from_index(opcode & 7);
    let carry = cpu.flags() & 1;
    let (result, flags) = alu::add16(cpu.wide(slot), 1);
    cpu.set_wide(slot, result);
    cpu.apply_result_flags(flags);
    cpu.set_carry(carry != 0);
    Outcome::Continue
}

fn push_all(cpu: &mut CpuState, ram: &mut GuestRam) -> Outcome {
    let saved = cpu.wide(Slot::Stack);
    let order = [Slot::Accumulator, Slot::Count, Slot::Data, Slot::Base];
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
    far_call(
        cpu,
        ram,
        u16::from_le_bytes([code_low, code_high]),
        u16::from_le_bytes([entry_low, entry_high]),
        opcode,
    )
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
    if !load_segment(
        cpu,
        ram,
        Segment::Code,
        u16::from_le_bytes([code_low, code_high]),
    ) {
        return Outcome::Fault(opcode);
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

fn absolute_move(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    segment: Option<Segment>,
    opcode: u8,
    wide: bool,
) -> Option<()> {
    let offset = if cpu.address32 {
        operand32::imm(cpu, ram)?
    } else {
        u32::from(u16::from_le_bytes([fetch(cpu, ram)?, fetch(cpu, ram)?]))
    };
    let operand = Operand::Memory {
        segment: segment.unwrap_or(Segment::Data),
        offset,
    };
    let ax = Slot::Accumulator;
    match opcode {
        0xa0 => {
            let byte = read_byte(cpu, ram, &operand)?;
            cpu.set_low(ax, byte);
        }
        0xa2 => write_byte(cpu, ram, &operand, cpu.low(ax)).then_some(())?,
        0xa1 if wide => {
            let value = operand32::read(cpu, ram, &operand)?;
            cpu.set_dword(ax, value);
        }
        0xa1 => {
            let value = read_word(cpu, ram, &operand)?;
            cpu.set_wide(ax, value);
        }
        0xa3 if wide => operand32::write(cpu, ram, &operand, cpu.dword(ax))?,
        0xa3 => write_word(cpu, ram, &operand, cpu.wide(ax)).then_some(())?,
        _ => return None,
    }
    Some(())
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
        if operation >= 4 {
            cpu.apply_result_flags(sized_flags(u16::from(value), 0x0080));
        }
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

fn bound_check(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    override_segment: Option<Segment>,
    opcode: u8,
) -> Outcome {
    let opcode_at = cpu.instruction().wrapping_sub(1);
    let Some(modrm) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(operand) = decode(cpu, ram, modrm, override_segment) else {
        return Outcome::Fault(opcode);
    };
    let Operand::Memory { segment, offset } = operand else {
        return Outcome::Fault(opcode);
    };
    let value = cpu.wide(Slot::from_index((modrm >> 3) & 7)) as i16;
    let selector = segment;
    let Some(lower) = load_pair(cpu, ram, selector, offset) else {
        return Outcome::Fault(opcode);
    };
    let Some(upper) = load_pair(cpu, ram, selector, offset.wrapping_add(2)) else {
        return Outcome::Fault(opcode);
    };
    if value < lower as i16 || value > upper as i16 {
        cpu.set_instruction(opcode_at);
        Outcome::Interrupt(5)
    } else {
        Outcome::Continue
    }
}

fn interrupt_on_overflow(cpu: &CpuState) -> Outcome {
    if cpu.flags() & 0x0800 != 0 {
        Outcome::Interrupt(4)
    } else {
        Outcome::Continue
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
    if !load_segment(cpu, ram, which, value) {
        return Outcome::Fault(opcode);
    }
    Outcome::Continue
}

fn return_far_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(low) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    return_far(cpu, ram, opcode, u16::from_le_bytes([low, high]))
}

pub(super) fn far_return(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    return_far(cpu, ram, 0xcb, 0) == Outcome::Continue
}

fn return_far(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8, discard: u16) -> Outcome {
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(selector) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(Slot::Stack, cpu.wide(Slot::Stack).wrapping_add(discard));
    #[cfg(feature = "pc386")]
    if cpu.machine.protected() && (selector & 3) > (cpu.selector(Segment::Code) & 3) {
        let (Some(sp), Some(ss)) = (pop_word(cpu, ram), pop_word(cpu, ram)) else {
            return Outcome::Fault(opcode);
        };
        if !load_segment(cpu, ram, Segment::Stack, ss) {
            return Outcome::Fault(opcode);
        }
        cpu.set_wide(Slot::Stack, sp.wrapping_add(discard));
    }
    cpu.set_instruction(next);
    if !load_segment(cpu, ram, Segment::Code, selector) {
        return Outcome::Fault(opcode);
    }
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
        4 | 6 => (value << 1, value & 0x8000 != 0),
        5 => (value >> 1, value & 1 != 0),
        7 => ((value as i16 >> 1) as u16, value & 1 != 0),
        _ => return Outcome::Fault(opcode),
    };
    if !write_word(cpu, ram, &operand, result) {
        return Outcome::Fault(opcode);
    }
    if operation >= 4 {
        cpu.apply_result_flags(sized_flags(result, 0x8000));
    }
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
            4 | 6 => (value << 1, value & 0x8000 != 0),
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
        if operation >= 4 {
            cpu.apply_result_flags(sized_flags(value, 0x8000));
        }
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
    cpu.set_carry(carry != 0);
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
            cpu.set_carry(carry != 0);
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
            let which = segment;
            let Some(entry_low) = cpu.load_byte(ram, which, offset) else {
                return Outcome::Fault(opcode);
            };
            let Some(entry_high) = cpu.load_byte(ram, which, offset.wrapping_add(1)) else {
                return Outcome::Fault(opcode);
            };
            let Some(code_low) = cpu.load_byte(ram, which, offset.wrapping_add(2)) else {
                return Outcome::Fault(opcode);
            };
            let Some(code_high) = cpu.load_byte(ram, which, offset.wrapping_add(3)) else {
                return Outcome::Fault(opcode);
            };
            if operation == 3 {
                return far_call(
                    cpu,
                    ram,
                    u16::from_le_bytes([code_low, code_high]),
                    u16::from_le_bytes([entry_low, entry_high]),
                    opcode,
                );
            }
            cpu.set_instruction(u16::from_le_bytes([entry_low, entry_high]));
            if !load_segment(
                cpu,
                ram,
                Segment::Code,
                u16::from_le_bytes([code_low, code_high]),
            ) {
                return Outcome::Fault(opcode);
            }
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
        Operand::Memory { offset, .. } => offset as u16,
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
            let product =
                i16::from(cpu.low(Slot::Accumulator) as i8).wrapping_mul(i16::from(value as i8));
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
            let Some(quotient) = dividend.checked_div(i16::from(divisor)) else {
                return Outcome::Fault(opcode);
            };
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
    if !push_word(cpu, ram, cpu.wide(Slot::Frame)) {
        return Outcome::Fault(opcode);
    }
    let frame = cpu.wide(Slot::Stack);
    if level > 0 {
        let mut depth = 1u8;
        while depth < level {
            let next = cpu.wide(Slot::Frame).wrapping_sub(2);
            cpu.set_wide(Slot::Frame, next);
            let Some(copied) = load_pair(cpu, ram, Segment::Stack, next) else {
                return Outcome::Fault(opcode);
            };
            if !push_word(cpu, ram, copied) {
                return Outcome::Fault(opcode);
            }
            depth += 1;
        }
        if !push_word(cpu, ram, frame) {
            return Outcome::Fault(opcode);
        }
    }
    cpu.set_wide(Slot::Frame, frame);
    let bytes = u16::from_le_bytes([low, high]);
    cpu.set_wide(Slot::Stack, cpu.wide(Slot::Stack).wrapping_sub(bytes));
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
        0 => (
            (value << 1) | u8::from(value & 0x80 != 0),
            value & 0x80 != 0,
        ),
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
    if operation >= 4 {
        cpu.apply_result_flags(sized_flags(u16::from(result), 0x0080));
    }
    cpu.set_carry(carried);
    note_shift_overflow(cpu, operation, u16::from(value), u16::from(result), 0x0080);
    Outcome::Continue
}

fn loop_count(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(byte) = fetch(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let count = if cpu.address32 {
        let n = cpu.dword(Slot::Count).wrapping_sub(1);
        cpu.set_dword(Slot::Count, n);
        n
    } else {
        let n = cpu.wide(Slot::Count).wrapping_sub(1);
        cpu.set_wide(Slot::Count, n);
        u32::from(n)
    };
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
    let which = segment;
    let Some(low) = cpu.load_byte(ram, which, offset) else {
        return Outcome::Fault(opcode);
    };
    let Some(high) = cpu.load_byte(ram, which, offset.wrapping_add(1)) else {
        return Outcome::Fault(opcode);
    };
    let Some(seg_low) = cpu.load_byte(ram, which, offset.wrapping_add(2)) else {
        return Outcome::Fault(opcode);
    };
    let Some(seg_high) = cpu.load_byte(ram, which, offset.wrapping_add(3)) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_wide(
        Slot::from_index((modrm >> 3) & 7),
        u16::from_le_bytes([low, high]),
    );
    if !load_segment(cpu, ram, target, u16::from_le_bytes([seg_low, seg_high])) {
        return Outcome::Fault(opcode);
    }
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

pub(super) fn interrupt_return(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let Some(next) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(selector) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    let Some(flags) = pop_word(cpu, ram) else {
        return Outcome::Fault(opcode);
    };
    #[cfg(feature = "pc386")]
    if cpu.machine.protected() && (selector & 3) > (cpu.selector(Segment::Code) & 3) {
        let (Some(sp), Some(ss)) = (pop_word(cpu, ram), pop_word(cpu, ram)) else {
            return Outcome::Fault(opcode);
        };
        if !load_segment(cpu, ram, Segment::Stack, ss) {
            return Outcome::Fault(opcode);
        }
        cpu.set_wide(Slot::Stack, sp);
    }
    cpu.set_instruction(next);
    if !load_segment(cpu, ram, Segment::Code, selector) {
        return Outcome::Fault(opcode);
    }
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
                4 | 6 => {
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
            if operation >= 4 {
                cpu.apply_result_flags(sized_flags(value, 0x8000));
            }
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
            if operation >= 4 {
                cpu.apply_result_flags(sized_flags(u16::from(value), 0x0080));
            }
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
    cpu.set_flags((cpu.flags() & !0x800) | u16::from(overflow) << 11);
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

fn set_byte_from_carry(cpu: &mut CpuState) -> Outcome {
    let filled = if cpu.flags() & 1 != 0 { 0xFF } else { 0 };
    cpu.set_low(Slot::Accumulator, filled);
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

fn translate(cpu: &mut CpuState, ram: &mut GuestRam, seg: Option<Segment>, opcode: u8) -> Outcome {
    let address = cpu
        .wide(Slot::Base)
        .wrapping_add(u16::from(cpu.low(Slot::Accumulator)));
    let Some(byte) = cpu.load_byte(ram, seg.unwrap_or(Segment::Data), address) else {
        return Outcome::Fault(opcode);
    };
    cpu.set_low(Slot::Accumulator, byte);
    Outcome::Continue
}

fn port_io(cpu: &mut CpuState, ram: &GuestRam, opcode: u8) -> Outcome {
    let port = if opcode & 8 == 0 {
        let Some(immediate) = fetch(cpu, ram) else {
            return Outcome::Fault(opcode);
        };
        u16::from(immediate)
    } else {
        cpu.wide(Slot::Data)
    };
    Outcome::Port {
        port,
        write: opcode & 2 != 0,
        word: opcode & 1 != 0,
    }
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
            let product =
                i32::from(cpu.wide(Slot::Accumulator) as i16).wrapping_mul(i32::from(value as i16));
            cpu.set_wide(Slot::Accumulator, product as u16);
            cpu.set_wide(Slot::Data, (product >> 16) as u16);
            note_multiply_overflow(cpu, product != i32::from(product as i16));
            Outcome::Continue
        }
        6 => {
            if value == 0 {
                return Outcome::Fault(opcode);
            }
            let dividend =
                (u32::from(cpu.wide(Slot::Data)) << 16) | u32::from(cpu.wide(Slot::Accumulator));
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
            let Some(quotient) = dividend.checked_div(i32::from(divisor)) else {
                return Outcome::Fault(opcode);
            };
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

fn load_pair(
    cpu: &CpuState,
    ram: &GuestRam,
    segment: Segment,
    offset: impl Into<u32>,
) -> Option<u16> {
    let offset = offset.into();
    let low = cpu.load_byte(ram, segment, offset)?;
    let high = cpu.load_byte(ram, segment, offset.wrapping_add(1))?;
    Some(u16::from_le_bytes([low, high]))
}

fn string_instruction(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    seg: Option<Segment>,
    op: u8,
    wide: bool,
    rep: Option<u8>,
    start: u16,
) -> Outcome {
    let width = if op & 1 == 0 {
        1u32
    } else if wide {
        4
    } else {
        2
    };
    let index = |c: &CpuState, r| {
        if c.address32 {
            c.dword(r)
        } else {
            u32::from(c.wide(r))
        }
    };
    let set = |c: &mut CpuState, r, v| {
        if c.address32 {
            c.set_dword(r, v)
        } else {
            c.set_wide(r, v as u16)
        }
    };
    let mut count = if rep.is_some() {
        index(cpu, Slot::Count)
    } else {
        1
    };
    let mut executed = 0;
    while count != 0 {
        if executed == 64 {
            cpu.set_instruction(start);
            break;
        }
        let si = index(cpu, Slot::Source);
        let di = index(cpu, Slot::Dest);
        let source = seg.unwrap_or(Segment::Data);
        let dest = Segment::Extra;
        let read = |s: Segment, offset: u32| -> Option<u32> {
            let mut v = 0;
            for i in 0..width {
                v |= u32::from(cpu.load_byte(ram, s, offset.wrapping_add(i))?) << (8 * i)
            }
            Some(v)
        };
        let has_src = matches!(op, 0xa4..=0xa7 | 0xac | 0xad);
        let mut a = cpu.dword(Slot::Accumulator);
        if has_src {
            let Some(v) = read(source, si) else {
                return Outcome::Fault(op);
            };
            a = v;
        }
        let cmp = matches!(op, 0xa6 | 0xa7 | 0xae | 0xaf);
        if cmp {
            let Some(b) = read(dest, di) else {
                return Outcome::Fault(op);
            };
            match width {
                1 => {
                    let (_, flags) = alu::sub8(a as u8, b as u8);
                    cpu.apply_result_flags(flags);
                }
                2 => {
                    let (_, flags) = alu::sub16(a as u16, b as u16);
                    cpu.apply_result_flags(flags);
                }
                _ => {
                    operand32::alu(cpu, 7, a, b);
                }
            }
        } else if matches!(op, 0xac | 0xad) {
            match width {
                1 => cpu.set_low(Slot::Accumulator, a as u8),
                2 => cpu.set_wide(Slot::Accumulator, a as u16),
                _ => cpu.set_dword(Slot::Accumulator, a),
            }
        } else {
            for i in 0..width {
                if !cpu.store_byte(ram, dest, di.wrapping_add(i), (a >> (8 * i)) as u8) {
                    return Outcome::Fault(op);
                }
            }
        }
        let delta = if cpu.direction_down() {
            width.wrapping_neg()
        } else {
            width
        };
        if has_src {
            set(cpu, Slot::Source, si.wrapping_add(delta));
        }
        if !matches!(op, 0xac | 0xad) {
            set(cpu, Slot::Dest, di.wrapping_add(delta));
        }
        count -= 1;
        if let Some(prefix) = rep {
            set(cpu, Slot::Count, count);
            if cmp && (cpu.flags() & 0x40 != 0) != (prefix == 0xf3) {
                break;
            }
        }
        executed += 1;
    }
    Outcome::Continue
}

fn input_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let dest = cpu.wide(Slot::Dest);
    let sample = if cpu.wide(Slot::Data) == 0 { b'Z' } else { 0 };
    if !cpu.store_byte(ram, Segment::Extra, dest, sample) {
        return Outcome::Fault(opcode);
    }
    cpu.set_wide(Slot::Dest, step_index(cpu, dest));
    Outcome::Continue
}

fn output_string(cpu: &mut CpuState, ram: &mut GuestRam, opcode: u8) -> Outcome {
    let source = cpu.wide(Slot::Source);
    if cpu.load_byte(ram, Segment::Data, source).is_none() {
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
    if cpu.address32 {
        let mut base = rm;
        let mut index = 0;
        if rm == 4 {
            let sib = fetch(cpu, ram)?;
            base = sib & 7;
            let i = (sib >> 3) & 7;
            if i != 4 {
                index = cpu
                    .dword(Slot::from_index(i))
                    .wrapping_shl(u32::from(sib >> 6));
            }
        }
        let none = mode == 0 && base == 5;
        let displacement = if none || mode == 2 {
            operand32::imm(cpu, ram)?
        } else if mode == 1 {
            fetch(cpu, ram)? as i8 as i32 as u32
        } else {
            0
        };
        let offset = index.wrapping_add(displacement).wrapping_add(if none {
            0
        } else {
            cpu.dword(Slot::from_index(base))
        });
        let segment = if !none && matches!(base, 4 | 5) {
            Segment::Stack
        } else {
            Segment::Data
        };
        return Some(Operand::Memory {
            segment: override_segment.unwrap_or(segment),
            offset,
        });
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
        offset: offset.into(),
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
        4 => (
            Segment::Data,
            cpu.wide(Slot::Source).wrapping_add(displacement),
        ),
        5 => (
            Segment::Data,
            cpu.wide(Slot::Dest).wrapping_add(displacement),
        ),
        6 => (
            Segment::Stack,
            cpu.wide(Slot::Frame).wrapping_add(displacement),
        ),
        _ => (
            Segment::Data,
            cpu.wide(Slot::Base).wrapping_add(displacement),
        ),
    }
}

fn read_word(cpu: &CpuState, ram: &GuestRam, operand: &Operand) -> Option<u16> {
    match *operand {
        Operand::Register(index) => Some(cpu.wide(Slot::from_index(index))),
        Operand::Memory { segment, offset } => {
            let low = cpu.load_byte(ram, segment, offset)?;
            let high = cpu.load_byte(ram, segment, offset.wrapping_add(1))?;
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
        Operand::Memory { segment, offset } => cpu.load_byte(ram, segment, offset),
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
            cpu.store_byte(ram, segment, offset, bytes[0])
                && cpu.store_byte(ram, segment, offset.wrapping_add(1), bytes[1])
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
        Operand::Memory { segment, offset } => cpu.store_byte(ram, segment, offset, value),
    }
}

fn byte_half(index: u8) -> (Slot, bool) {
    let high = index & 7 >= 4;
    (Slot::from_index(index & 3), high)
}

pub(super) fn push_word(cpu: &mut CpuState, ram: &mut GuestRam, value: u16) -> bool {
    let pointer = cpu.wide(Slot::Stack).wrapping_sub(2);
    cpu.set_wide(Slot::Stack, pointer);
    let segment = Segment::Stack;
    let bytes = value.to_le_bytes();
    cpu.store_byte(ram, segment, pointer, bytes[0])
        && cpu.store_byte(ram, segment, pointer.wrapping_add(1), bytes[1])
}

fn pop_word(cpu: &mut CpuState, ram: &GuestRam) -> Option<u16> {
    let pointer = cpu.wide(Slot::Stack);
    let segment = Segment::Stack;
    let low = cpu.load_byte(ram, segment, pointer)?;
    let high = cpu.load_byte(ram, segment, pointer.wrapping_add(1))?;
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
    let byte = cpu.load_byte(ram, Segment::Code, cpu.instruction())?;
    cpu.set_instruction(cpu.instruction().wrapping_add(1));
    Some(byte)
}

fn load_segment(cpu: &mut CpuState, ram: &GuestRam, seg: Segment, selector: u16) -> bool {
    #[cfg(feature = "pc386")]
    if !cpu.machine.load(ram, seg, selector) {
        return false;
    }
    #[cfg(not(feature = "pc386"))]
    let _ = ram;
    cpu.set_selector(seg, selector);
    true
}

fn far_call(cpu: &mut CpuState, ram: &mut GuestRam, sel: u16, ip: u16, opcode: u8) -> Outcome {
    #[cfg(not(feature = "pc386"))]
    let target = sel;
    #[cfg(feature = "pc386")]
    let (mut target, mut ip) = (sel, ip);
    let caller = cpu.selector(Segment::Code);
    let next = cpu.instruction();
    #[cfg(feature = "pc386")]
    if cpu.machine.protected() {
        let Some(d) = cpu.machine.descriptor(ram, sel) else {
            return Outcome::Fault(opcode);
        };
        let cpl = caller & 3;
        let dpl = u16::from(d.access >> 5 & 3);
        if d.access & 16 == 0 {
            if (sel & 3).max(cpl) > dpl {
                cpu.machine.exception.set(Some((13, Some(sel & !3))));
                return Outcome::Fault(opcode);
            }
            let Some((cs, entry, args)) = cpu.machine.call_gate(ram, sel) else {
                return Outcome::Fault(opcode);
            };
            let Some(code) = cpu.machine.descriptor(ram, cs) else {
                return Outcome::Fault(opcode);
            };
            let level = u16::from((code.access >> 5) & 3);
            if code.access & 0x98 != 0x98 || level > cpl {
                cpu.machine.exception.set(Some((13, Some(cs & !3))));
                return Outcome::Fault(opcode);
            }
            let level = if code.access & 4 != 0 { cpl } else { level };
            if level < cpl && !super::pc386::switch_stack(cpu, ram, level, args) {
                return Outcome::Fault(opcode);
            }
            target = cs & !3 | level;
            ip = entry;
        } else {
            if d.access & 8 == 0
                || if d.access & 4 != 0 {
                    dpl > cpl
                } else {
                    dpl != cpl || sel & 3 > cpl
                }
            {
                cpu.machine.exception.set(Some((13, Some(sel & !3))));
                return Outcome::Fault(opcode);
            }
            target = sel & !3 | cpl;
        }
    }
    if !push_word(cpu, ram, caller)
        || !push_word(cpu, ram, next)
        || !load_segment(cpu, ram, Segment::Code, target)
    {
        return Outcome::Fault(opcode);
    }
    cpu.set_instruction(ip);
    Outcome::Continue
}
