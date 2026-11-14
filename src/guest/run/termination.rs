//! Real-mode DOS termination vectors and the INT 21h user register frame.
use super::{CpuState, GuestRam, Segment, Session, Slot, read_word, store_bytes};

pub(super) type Address = (u16, u16); // segment, offset

fn pointer(ram: &GuestRam, seg: u16, off: u16) -> Address {
    (
        read_word(ram, seg, off.wrapping_add(2)).unwrap_or(0),
        read_word(ram, seg, off).unwrap_or(0),
    )
}

fn put_pointer(ram: &mut GuestRam, seg: u16, off: u16, (s, o): Address) {
    store_bytes(ram, seg, off, &o.to_le_bytes());
    store_bytes(ram, seg, off.wrapping_add(2), &s.to_le_bytes());
}

pub(super) fn vector(session: &mut Session, ram: &mut GuestRam, n: u8, address: Address) {
    let id = usize::from(n);
    (session.vector_segment[id], session.vector_offset[id]) = address;
    session.vector_live[id] = address != (0, 0);
    put_pointer(ram, 0, u16::from(n) * 4, address);
}

pub(super) fn save_vectors(ram: &mut GuestRam, psp: u16) {
    for n in 0x22..=0x24 {
        let address = pointer(ram, 0, n * 4);
        put_pointer(ram, psp, 10 + (n - 0x22) * 4, address);
    }
}

const GENERAL: [Slot; 7] = [
    Slot::Accumulator,
    Slot::Base,
    Slot::Count,
    Slot::Data,
    Slot::Source,
    Slot::Dest,
    Slot::Frame,
];
const FRAME_SIZE: u16 = 24;

pub(super) fn save_stack(cpu: &CpuState, ram: &mut GuestRam, psp: u16) {
    let ss = cpu.selector(Segment::Stack);
    let sp = cpu.wide(Slot::Stack).wrapping_sub(FRAME_SIZE);
    put_pointer(ram, psp, 0x2e, (ss, sp));
    let tail = [
        cpu.selector(Segment::Data),
        cpu.selector(Segment::Extra),
        cpu.instruction(),
        cpu.selector(Segment::Code),
        cpu.flags(),
    ];
    for (i, word) in GENERAL
        .map(|s| cpu.wide(s))
        .into_iter()
        .chain(tail)
        .enumerate()
    {
        store_bytes(ram, ss, sp.wrapping_add(2 * i as u16), &word.to_le_bytes());
    }
}

fn restore_stack(cpu: &mut CpuState, ram: &GuestRam, psp: u16) {
    let (ss, sp) = pointer(ram, psp, 0x2e);
    if (ss, sp) == (0, 0) {
        return;
    }
    let word = |i| read_word(ram, ss, sp.wrapping_add(2 * i)).unwrap_or(0);
    for (i, s) in GENERAL.into_iter().enumerate() {
        cpu.set_wide(s, word(i as u16));
    }
    cpu.set_selector(Segment::Data, word(7));
    cpu.set_selector(Segment::Extra, word(8));
    cpu.set_selector(Segment::Stack, ss);
    cpu.set_wide(Slot::Stack, sp.wrapping_add(FRAME_SIZE));
}

pub(super) fn resume(cpu: &mut CpuState, target: Address) {
    cpu.set_selector(Segment::Code, target.0);
    cpu.set_instruction(target.1);
    cpu.set_flags(0x202);
}

/// A PSP created with AH=55h has no kernel EXEC frame. Its parent lives inside
/// the same machine, including its protected-mode descriptors and device state.
pub(super) fn exit(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    session: &mut Session,
    code: u8,
) -> Option<super::End> {
    let psp = session.current_psp;
    let parent = read_word(ram, psp, 0x16).unwrap_or(0);
    let target = pointer(ram, psp, 10);
    for n in 0x22..=0x24 {
        vector(
            session,
            ram,
            n,
            pointer(ram, psp, 10 + u16::from(n - 0x22) * 4),
        );
    }
    if psp != session.prefix && parent != 0 && parent != psp && target != (0, 0) {
        restore_stack(cpu, ram, parent);
        resume(cpu, target);
        session.current_psp = parent;
        session.last_code = u16::from(code);
        session.arena.release_process(psp);
        session.arena.publish(ram);
        return None;
    }
    Some(super::End::Stopped(code))
}

pub(super) fn continuation(ram: &GuestRam, psp: u16) -> Option<Address> {
    let target = pointer(ram, psp, 10);
    (target != (0, 0)).then_some(target)
}
