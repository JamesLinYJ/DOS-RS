//! Load programs from the guest volume and run them.
//!
//! `HELLO.COM` adds six to the letter A and prints the tail. `HELLO.EXE`
//! prints after its relocation is applied. `RUN.COM` reads `DATA.TXT`,
//! executes `CHILD.COM`, and prints that child's return code.

use super::memory::{self, GuestRam};
use super::state::{CpuState, Segment, Slot};
use super::step::{self, Outcome};
use super::volume;

const COM_SEGMENT: u16 = 0x0100;
const EXE_PREFIX: u16 = 0x0300;
const RUN_SEGMENT: u16 = 0x0500;
const CHILD_SEGMENT: u16 = 0x0700;

pub fn show() {
    show_image(b"HELLO   COM", COM_SEGMENT, false);
    show_image(b"HELLO   EXE", EXE_PREFIX, true);
    show_image(b"RUN     COM", RUN_SEGMENT, false);
}

fn show_image(name: &[u8; 11], segment: u16, executable: bool) {
    let mut image = [0u8; 512];
    let Some(length) = volume::read_named(volume::IMAGE, name, &mut image) else {
        crate::bios::mscon::con_write_bytes(b"GUEST MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    let mut session = Session::new();
    let Some(mut cpu) = place(&image[..length], ram, segment, executable) else {
        crate::bios::mscon::con_write_bytes(b"GUEST FAULT\r\n");
        return;
    };
    match drive(&mut cpu, ram, &mut session, 128) {
        End::Stopped(_) => {}
        End::Fault(opcode) => {
            crate::bios::mscon::con_write_bytes(b"GUEST FAULT ");
            put_hex8(opcode);
            crate::bios::mscon::con_write_bytes(b"\r\n");
        }
        End::Other => crate::bios::mscon::con_write_bytes(b"GUEST INT\r\n"),
        End::Loop => crate::bios::mscon::con_write_bytes(b"GUEST LOOP\r\n"),
    }
}

fn place(image: &[u8], ram: &mut GuestRam, segment: u16, executable: bool) -> Option<CpuState> {
    if executable && image.first() == Some(&b'M') && image.get(1) == Some(&b'Z') {
        return super::mz::load(image, ram, segment);
    }
    if !ram.store(segment, 0, 0xCD) || !ram.store(segment, 1, 0x20) {
        return None;
    }
    let mut offset = 0x100u16;
    for byte in image {
        if !ram.store(segment, offset, *byte) {
            return None;
        }
        offset = offset.wrapping_add(1);
    }
    Some(CpuState::at_com(segment))
}

enum End {
    Stopped(u8),
    Fault(u8),
    Other,
    Loop,
}

fn drive(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session, limit: u16) -> End {
    let mut steps = 0u16;
    while steps < limit {
        steps += 1;
        match step::step(cpu, ram) {
            Outcome::Continue => {}
            Outcome::Interrupt(0x21) => match service(cpu, ram, session) {
                Action::Continue => {}
                Action::Exit(code) => return End::Stopped(code),
            },
            Outcome::Interrupt(0x20) | Outcome::Halt => return End::Stopped(0),
            Outcome::Fault(opcode) => return End::Fault(opcode),
            Outcome::Interrupt(_) => return End::Other,
        }
    }
    End::Loop
}

enum Action {
    Continue,
    Exit(u8),
}

struct Opened {
    bytes: [u8; 128],
    len: usize,
    pos: usize,
}

struct Session {
    opens: [Option<Opened>; 4],
    last_code: u16,
}

impl Session {
    fn new() -> Self {
        Self {
            opens: [None, None, None, None],
            last_code: 0,
        }
    }
}

fn service(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) -> Action {
    match cpu.high(Slot::Accumulator) {
        0x02 => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Data)]);
            Action::Continue
        }
        0x09 => {
            print_marked(cpu, ram);
            Action::Continue
        }
        0x3D => {
            open_file(cpu, ram, session);
            Action::Continue
        }
        0x3E => {
            close_file(cpu, session);
            Action::Continue
        }
        0x3F => {
            read_file(cpu, ram, session);
            Action::Continue
        }
        0x40 => {
            write_console(cpu, ram);
            Action::Continue
        }
        0x4B => {
            launch(cpu, ram, session);
            Action::Continue
        }
        0x4C => Action::Exit(cpu.low(Slot::Accumulator)),
        0x4D => {
            cpu.set_wide(Slot::Accumulator, session.last_code);
            session.last_code = 0;
            cpu.set_carry(false);
            Action::Continue
        }
        _ => {
            cpu.set_carry(true);
            Action::Continue
        }
    }
}

fn open_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let Some(name) = dos_name(&guest_text(cpu, ram)) else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    let mut file = [0u8; 128];
    let Some(length) = volume::read_named(volume::IMAGE, &name, &mut file) else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    let mut slot = 0usize;
    while slot < session.opens.len() {
        if session.opens[slot].is_none() {
            session.opens[slot] = Some(Opened {
                bytes: file,
                len: length,
                pos: 0,
            });
            cpu.set_wide(Slot::Accumulator, (slot as u16).wrapping_add(3));
            cpu.set_carry(false);
            return;
        }
        slot += 1;
    }
    cpu.set_wide(Slot::Accumulator, 4);
    cpu.set_carry(true);
}

fn close_file(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle >= 3 {
        let index = usize::from(handle - 3);
        if let Some(slot) = session.opens.get_mut(index) {
            *slot = None;
            cpu.set_carry(false);
            return;
        }
    }
    cpu.set_carry(true);
}

fn read_file(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    let requested = usize::from(cpu.wide(Slot::Count));
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
    if handle < 3 {
        cpu.set_carry(true);
        return;
    }
    let Some(Some(opened)) = session.opens.get_mut(usize::from(handle - 3)) else {
        cpu.set_carry(true);
        return;
    };
    let mut moved = 0u16;
    while usize::from(moved) < requested && opened.pos < opened.len {
        let byte = opened.bytes[opened.pos];
        if !ram.store(segment, offset, byte) {
            break;
        }
        opened.pos += 1;
        offset = offset.wrapping_add(1);
        moved += 1;
    }
    cpu.set_wide(Slot::Accumulator, moved);
    cpu.set_carry(false);
}

fn write_console(cpu: &mut CpuState, ram: &GuestRam) {
    if cpu.wide(Slot::Base) != 1 {
        cpu.set_carry(true);
        return;
    }
    let count = cpu.wide(Slot::Count);
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut text = [0u8; 80];
    let mut length = 0usize;
    let mut seen = 0u16;
    while seen < count && length < text.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        text[length] = byte;
        length += 1;
        seen += 1;
        offset = offset.wrapping_add(1);
    }
    crate::bios::mscon::con_write_bytes(&text[..length]);
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

fn launch(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        cpu.set_carry(true);
        return;
    }
    let Some(name) = dos_name(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    let mut image = [0u8; 512];
    let Some(length) = volume::read_named(volume::IMAGE, &name, &mut image) else {
        cpu.set_carry(true);
        cpu.set_wide(Slot::Accumulator, 2);
        return;
    };
    let executable = image.first() == Some(&b'M') && image.get(1) == Some(&b'Z');
    let Some(mut child) = place_child(&image[..length], ram, executable) else {
        cpu.set_carry(true);
        return;
    };
    match drive(&mut child, ram, session, 64) {
        End::Stopped(code) => {
            session.last_code = u16::from(code);
            cpu.set_carry(false);
        }
        _ => cpu.set_carry(true),
    }
}

fn place_child(image: &[u8], ram: &mut GuestRam, executable: bool) -> Option<CpuState> {
    if executable {
        super::mz::load(image, ram, CHILD_SEGMENT)
    } else {
        place_com(image, ram, CHILD_SEGMENT)
    }
}

fn place_com(image: &[u8], ram: &mut GuestRam, segment: u16) -> Option<CpuState> {
    if !ram.store(segment, 0, 0xCD) || !ram.store(segment, 1, 0x20) {
        return None;
    }
    let mut offset = 0x100u16;
    for byte in image {
        if !ram.store(segment, offset, *byte) {
            return None;
        }
        offset = offset.wrapping_add(1);
    }
    Some(CpuState::at_com(segment))
}

fn guest_text(cpu: &CpuState, ram: &GuestRam) -> [u8; 64] {
    let mut text = [0u8; 64];
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
    let mut index = 0usize;
    while index < text.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == 0 {
            break;
        }
        text[index] = byte;
        index += 1;
        offset = offset.wrapping_add(1);
    }
    text
}

fn dos_name(text: &[u8; 64]) -> Option<[u8; 11]> {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut seen = false;
    for byte in text {
        if *byte == 0 {
            break;
        }
        seen = true;
        if *byte == b'.' && !extension {
            extension = true;
            continue;
        }
        let upper = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            *byte
        };
        if !extension && base < 8 {
            name[base] = upper;
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = upper;
            ext += 1;
        }
    }
    if seen { Some(name) } else { None }
}

fn print_marked(cpu: &CpuState, ram: &GuestRam) {
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
