//! Run the kernel's guest CPU and devices directly on a host Rust toolchain.
#![allow(dead_code)]
extern crate alloc;

#[path = "../src/guest/alu.rs"]
mod alu;
#[path = "../src/guest/at.rs"]
mod at;
#[path = "../src/kernel_cell.rs"]
mod kernel_cell;
#[path = "../src/guest/memory.rs"]
mod memory;
#[path = "../src/guest/pc386.rs"]
mod pc386;
#[path = "../src/guest/state.rs"]
mod state;
#[path = "../src/guest/step.rs"]
mod step;
#[path = "../src/guest/vga.rs"]
mod vga;

mod bios {
    pub mod screen {
        pub fn graphics(_: usize, _: usize, _: &[u8]) {}
        pub fn glyph(_: u8) -> [u8; 8] {
            [0; 8]
        }
    }
}
mod run {
    pub mod xms {
        pub const CONTROL: (u16, u16) = (0xf000, 0x105);
    }
    pub mod ems {
        pub const ENTRY: (u16, u16) = (0xf100, 0);
        pub const RETURN: (u16, u16) = (0xf100, 0x20);
    }
}

use memory::GuestRam;
use state::{CpuState, Segment, Slot};
use step::Outcome;

fn bytes(ram: &mut GuestRam, base: usize, data: &[u8]) {
    for (i, &byte) in data.iter().enumerate() {
        ram.set_bus_byte(base + i, byte);
    }
}

#[test]
fn protected_far_jump_interrupt_and_iret_use_guest_tables() {
    let mut ram = memory::session_ram(0);
    ram.clear();
    let mut cpu = CpuState::at_com(0);
    bytes(
        &mut ram,
        0x100,
        &[
            0x0f, 0x01, 0x16, 0, 2, // lgdt [200h]
            0x0f, 0x01, 0x1e, 6, 2, // lidt [206h]
            0xb8, 1, 0, 0x0f, 0x01, 0xf0, // mov ax,1; lmsw ax
            0xea, 0x40, 0, 8, 0, // jmp 0008:0040
        ],
    );
    bytes(&mut ram, 0x200, &[23, 0, 0, 3, 0, 0, 0x0f, 1, 0, 4, 0, 0]);
    bytes(&mut ram, 0x308, &[0xff, 0xff, 0, 0x50, 0, 0x9b, 0, 0]);
    bytes(&mut ram, 0x310, &[0xff, 0xff, 0, 0x60, 0, 0x93, 0, 0]);
    bytes(&mut ram, 0x508, &[0x70, 0, 8, 0, 0, 0x86, 0, 0]); // vector 21h
    bytes(
        &mut ram,
        0x5040,
        &[0xb8, 16, 0, 0x8e, 0xd0, 0xcd, 0x21, 0xf4],
    );
    bytes(&mut ram, 0x5070, &[0xb8, 0x34, 0x12, 0xcf]);
    assert_eq!(step::slice(&mut cpu, &mut ram, 16).0, Outcome::Halt);
    assert!(cpu.machine.protected());
    assert_eq!(cpu.selector(Segment::Code), 8);
    assert_eq!(cpu.selector(Segment::Stack), 16);
    assert_eq!(cpu.wide(Slot::Stack), 0x400);
    assert_eq!(cpu.wide(Slot::Accumulator), 0x1234);
    assert_ne!(cpu.flags() & 0x200, 0);
    assert!(cpu.machine.load(&ram, Segment::Data, 0));
    assert!(cpu.load_byte(&ram, Segment::Data, 0u16).is_none());
    assert_eq!(cpu.machine.exception.get(), Some((13, Some(0))));
}

#[test]
fn rotates_preserve_zero_and_inc_preserves_carry() {
    let mut ram = memory::session_ram(1);
    ram.clear();
    let mut cpu = CpuState::at_com(0);
    bytes(&mut ram, 0x100, &[0x40, 0xd1, 0xc8, 0xf4]); // INC AX; ROR AX,1
    cpu.set_wide(Slot::Accumulator, 0xffff);
    cpu.set_carry(true);
    assert_eq!(step::step(&mut cpu, &mut ram), Outcome::Continue);
    assert_eq!(cpu.flags() & 0x41, 0x41);
    cpu.set_wide(Slot::Accumulator, 1);
    assert_eq!(step::step(&mut cpu, &mut ram), Outcome::Continue);
    assert_eq!(cpu.wide(Slot::Accumulator), 0x8000);
    assert_eq!(cpu.flags() & 0x41, 0x41);
}

#[test]
fn double_shift_long_division_and_absolute_address_override() {
    let mut ram = memory::session_ram(2);
    ram.clear();
    let mut cpu = CpuState::at_com(0);
    cpu.set_dword(Slot::Accumulator, 0x80000000);
    cpu.set_dword(Slot::Data, 0x40000000);
    bytes(
        &mut ram,
        0x100,
        &[
            0x66, 0x0f, 0xa4, 0xd0, 2, // SHLD EAX,EDX,2
            0x66, 0xf7, 0xf1, // DIV ECX
            0x67, 0x66, 0xa3, 0, 8, 0, 0, 0xf4,
        ],
    ); // MOV [00000800h],EAX
    assert_eq!(step::step(&mut cpu, &mut ram), Outcome::Continue);
    assert_eq!(cpu.dword(Slot::Accumulator), 1);
    cpu.set_dword(Slot::Data, 1);
    cpu.set_dword(Slot::Count, 2);
    assert_eq!(step::slice(&mut cpu, &mut ram, 3).0, Outcome::Halt);
    assert_eq!(cpu.dword(Slot::Accumulator), 0x80000000);
    assert_eq!(cpu.dword(Slot::Data), 1);
    assert_eq!(ram.bus_byte(0x803), 0x80);
}

#[test]
fn keyboard_irq_waits_for_eoi_and_vga_latches_copy_all_planes() {
    let mut at = at::At::new();
    let mut a20 = false;
    at.key(0x1e);
    at.key(0x9e);
    assert_eq!(at.interrupt(), Some(9));
    assert_eq!(at.port(0x60, false, 0, &mut a20), Some(0x1e));
    assert_eq!(at.interrupt(), None);
    at.eoi();
    assert_eq!(at.interrupt(), Some(9));
    assert_eq!(at.port(0x60, false, 0, &mut a20), Some(0x9e));
    at.advance(65536);
    assert_eq!(at.interrupt(), Some(8));
    let mut video = vga::Graphics::new();
    video.planar_mode(true);
    for plane in 0..4 {
        video.write(0x3c4, 2);
        video.write(0x3c5, 1 << plane);
        video.store(10, 0xf0 | plane);
    }
    video.byte(10);
    video.write(0x3c5, 15);
    video.write(0x3ce, 5);
    video.write(0x3cf, 1);
    video.store(11, 0);
    for plane in 0..4 {
        video.write(0x3ce, 4);
        video.write(0x3cf, plane);
        assert_eq!(video.byte(11), 0xf0 | plane);
    }
}

#[test]
fn call_gate_copies_parameters_and_retf_releases_both_stacks() {
    let mut ram = memory::session_ram(3);
    ram.clear();
    let mut cpu = CpuState::at_com(0);
    cpu.machine.gdtr = pc386::Table {
        base: 0x300,
        limit: 55,
    };
    for (selector, base, access) in [
        (8, 0x50, 0x9b),
        (16, 0x60, 0x93),
        (24, 0x70, 0xfb),
        (32, 0x80, 0xf3),
        (40, 0x90, 0x83),
    ] {
        bytes(
            &mut ram,
            0x300 + selector,
            &[0xff, 0xff, 0, base, 0, access, 0, 0],
        );
    }
    bytes(&mut ram, 0x330, &[0x10, 0, 8, 0, 1, 0xe4, 0, 0]);
    bytes(&mut ram, 0x9002, &[0, 3, 16, 0]); // TSS ring-0 SP and SS
    bytes(&mut ram, 0x5010, &[0x89, 0xe5, 0x8b, 0x46, 4, 0xca, 2, 0]); // mov bp,sp; mov ax,[bp+4]; retf 2
    bytes(
        &mut ram,
        0x7100,
        &[0x68, 0x34, 0x12, 0x9a, 0, 0, 0x33, 0, 0xf4],
    );
    cpu.machine.cr0 = 1;
    cpu.machine.tr = 40;
    for (reg, sel) in [(Segment::Code, 27), (Segment::Stack, 35)] {
        assert!(cpu.machine.load(&ram, reg, sel));
        cpu.set_selector(reg, sel);
    }
    assert_eq!(step::slice(&mut cpu, &mut ram, 12).0, Outcome::Halt);
    assert_eq!(cpu.wide(Slot::Accumulator), 0x1234);
    assert_eq!(cpu.wide(Slot::Stack), 0x400);
    assert_eq!(cpu.selector(Segment::Stack), 35);
    assert_eq!(cpu.selector(Segment::Code), 27);
}
