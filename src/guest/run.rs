//! Load `HELLO.COM` from the guest volume and run it.
//!
//! The program adds six to the letter A, prints that byte, then prints the
//! dollar-terminated tail. The two writes are `GUEST HELLO COM`.

use super::memory;
use super::state::{CpuState, Segment, Slot};
use super::step::{self, Outcome};
use super::volume;

const PROGRAM_SEGMENT: u16 = 0x1000;

pub fn show() {
    let mut program = [0u8; 256];
    let Some(length) = volume::read_named(volume::IMAGE, b"HELLO   COM", &mut program) else {
        crate::bios::mscon::con_write_bytes(b"GUEST MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    if !ram.store(PROGRAM_SEGMENT, 0, 0xCD) || !ram.store(PROGRAM_SEGMENT, 1, 0x20) {
        crate::bios::mscon::con_write_bytes(b"GUEST FAULT\r\n");
        return;
    }
    let mut offset = 0x100u16;
    for byte in program[..length].iter() {
        if !ram.store(PROGRAM_SEGMENT, offset, *byte) {
            crate::bios::mscon::con_write_bytes(b"GUEST FAULT\r\n");
            return;
        }
        offset = offset.wrapping_add(1);
    }
    let mut cpu = CpuState::at_com(PROGRAM_SEGMENT);
    let mut steps = 0u16;
    while steps < 64 {
        steps += 1;
        match step::step(&mut cpu, ram) {
            Outcome::Continue => {}
            Outcome::Interrupt(0x21) => {
                if !service(&mut cpu, ram) {
                    return;
                }
            }
            Outcome::Interrupt(0x20) | Outcome::Halt => return,
            Outcome::Fault(opcode) => {
                crate::bios::mscon::con_write_bytes(b"GUEST FAULT ");
                put_hex8(opcode);
                crate::bios::mscon::con_write_bytes(b"\r\n");
                return;
            }
            Outcome::Interrupt(_) => {
                crate::bios::mscon::con_write_bytes(b"GUEST INT\r\n");
                return;
            }
        }
    }
    crate::bios::mscon::con_write_bytes(b"GUEST LOOP\r\n");
}

fn service(cpu: &mut CpuState, ram: &mut memory::GuestRam) -> bool {
    match cpu.high(Slot::Accumulator) {
        0x02 => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Data)]);
            true
        }
        0x09 => {
            print_marked(cpu, ram);
            true
        }
        0x4C => false,
        _ => true,
    }
}

fn print_marked(cpu: &CpuState, ram: &memory::GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut text = [0u8; 80];
    let mut length = 0usize;
    while length < text.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == b'$' {
            break;
        }
        text[length] = byte;
        length += 1;
        offset = offset.wrapping_add(1);
    }
    crate::bios::mscon::con_write_bytes(&text[..length]);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0F) as usize],
    ]);
}
