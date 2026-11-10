//! Load programs from the guest volume and run them.
//!
//! `HELLO.COM` adds six to the letter A and prints the tail. `HELLO.EXE`
//! prints after its relocation is applied. `RUN.COM` reads `DATA.TXT`,
//! executes `CHILD.COM`, and prints that child's return code. `OPCHECK.COM`,
//! `DOSCALL.COM`, and `BIOSCALL.COM` exercise later opcodes and services.

use super::memory::{self, GuestRam};
use super::state::{CpuState, Segment, Slot};
use super::step::{self, Outcome};
use super::volume;

const COM_SEGMENT: u16 = 0x0100;
const EXE_PREFIX: u16 = 0x0300;
const RUN_SEGMENT: u16 = 0x0500;
const CHILD_SEGMENT: u16 = 0x0700;
const SHELL_SEGMENT: u16 = 0x0900;
const SHELL_CHILD: u16 = 0x0B00;
const WIDE_SEGMENT: u16 = 0x0D00;
const GUEST_BLOCK: u16 = 0x0C00;

pub fn show() {
    show_image(b"HELLO   COM", COM_SEGMENT, false, 128);
    show_image(b"HELLO   EXE", EXE_PREFIX, true, 128);
    show_image(b"RUN     COM", RUN_SEGMENT, false, 128);
    show_shell();
    show_image(b"OPCHECK COM", WIDE_SEGMENT, false, 8192);
    super::disk::boot();
    show_image(b"DOSCALL COM", WIDE_SEGMENT, false, 8192);
    show_image(b"BIOSCALLCOM", WIDE_SEGMENT, false, 8192);
}

fn show_shell() {
    super::disk::boot();
    super::command::reset();
    let mut image = [0u8; 512];
    let Some(length) = volume::read_named(volume::IMAGE, b"COMMAND COM", &mut image) else {
        crate::bios::mscon::con_write_bytes(b"CMD MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    let mut session = Session::new();
    let Some(mut cpu) = place(&image[..length], ram, SHELL_SEGMENT, false) else {
        crate::bios::mscon::con_write_bytes(b"CMD FAULT\r\n");
        return;
    };
    match drive(&mut cpu, ram, &mut session, 20000) {
        End::Stopped(_) => {}
        End::Fault(opcode) => {
            crate::bios::mscon::con_write_bytes(b"CMD FAULT ");
            put_hex8(opcode);
            crate::bios::mscon::con_write_bytes(b"\r\n");
        }
        End::Other => crate::bios::mscon::con_write_bytes(b"CMD INT\r\n"),
        End::Loop => crate::bios::mscon::con_write_bytes(b"CMD LOOP\r\n"),
    }
}

fn show_image(name: &[u8; 11], segment: u16, executable: bool, limit: u16) {
    let mut image = [0u8; 1024];
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
    match drive(&mut cpu, ram, &mut session, limit) {
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
            Outcome::Interrupt(0x2E) => super::command::invoke(cpu, ram, session),
            Outcome::Interrupt(0x21) => match service(cpu, ram, session) {
                Action::Continue => {}
                Action::Exit(code) => return End::Stopped(code),
            },
            Outcome::Interrupt(0x20) | Outcome::Halt => return End::Stopped(0),
            Outcome::Fault(opcode) => return End::Fault(opcode),
            Outcome::Interrupt(vector) => {
                if !serve_bios(cpu, ram, vector) {
                    return End::Other;
                }
            }
        }
    }
    End::Loop
}

enum Action {
    Continue,
    Exit(u8),
}

struct Opened {
    bytes: [u8; 512],
    len: usize,
    pos: usize,
}

pub(super) struct Session {
    opens: [Option<Opened>; 4],
    last_code: u16,
    dta_segment: u16,
    dta_offset: u16,
    block: u16,
    vector_id: u8,
    vector_segment: u16,
    vector_offset: u16,
    vector_live: bool,
}

impl Session {
    fn new() -> Self {
        Self {
            opens: [None, None, None, None],
            last_code: 0,
            dta_segment: 0,
            dta_offset: 0,
            block: 0,
            vector_id: 0,
            vector_segment: 0,
            vector_offset: 0,
            vector_live: false,
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
        0x19 => {
            cpu.set_low(Slot::Accumulator, 0);
            cpu.set_carry(false);
            Action::Continue
        }
        0x1A => {
            session.dta_segment = cpu.selector(Segment::Data);
            session.dta_offset = cpu.wide(Slot::Data);
            cpu.set_carry(false);
            Action::Continue
        }
        0x25 => {
            session.vector_id = cpu.low(Slot::Accumulator);
            session.vector_segment = cpu.selector(Segment::Data);
            session.vector_offset = cpu.wide(Slot::Data);
            session.vector_live = true;
            cpu.set_carry(false);
            Action::Continue
        }
        0x2A => {
            cpu.set_wide(Slot::Count, 1980);
            cpu.set_high(Slot::Data, 1);
            cpu.set_low(Slot::Data, 1);
            cpu.set_low(Slot::Accumulator, 2);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2C => {
            cpu.set_wide(Slot::Count, 0);
            cpu.set_wide(Slot::Data, 0);
            cpu.set_carry(false);
            Action::Continue
        }
        0x30 => {
            cpu.set_wide(Slot::Accumulator, 0x0004);
            cpu.set_carry(false);
            Action::Continue
        }
        0x35 => {
            if session.vector_live && session.vector_id == cpu.low(Slot::Accumulator) {
                cpu.set_selector(Segment::Extra, session.vector_segment);
                cpu.set_wide(Slot::Base, session.vector_offset);
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
            Action::Continue
        }
        0x36 => {
            report_space(cpu);
            Action::Continue
        }
        0x39 => {
            cpu.set_carry(!named(cpu, ram, super::disk::make_dir));
            Action::Continue
        }
        0x3A => {
            cpu.set_carry(!named(cpu, ram, super::disk::remove_dir));
            Action::Continue
        }
        0x3B => {
            cpu.set_carry(!change_directory(cpu, ram));
            Action::Continue
        }
        0x3C => {
            cpu.set_carry(!named(cpu, ram, |name| super::disk::write_file(name, b"")));
            Action::Continue
        }
        0x41 => {
            cpu.set_carry(!named(cpu, ram, super::disk::remove_file));
            Action::Continue
        }
        0x42 => {
            seek_file(cpu, session);
            Action::Continue
        }
        0x43 => {
            read_attribute(cpu, ram);
            Action::Continue
        }
        0x47 => {
            write_root(cpu, ram);
            Action::Continue
        }
        0x48 => {
            let asked = cpu.wide(Slot::Base);
            if asked == 0 || asked > 16 {
                cpu.set_wide(Slot::Base, 16);
                cpu.set_wide(Slot::Accumulator, 8);
                cpu.set_carry(true);
            } else {
                session.block = GUEST_BLOCK;
                cpu.set_wide(Slot::Accumulator, GUEST_BLOCK);
                cpu.set_carry(false);
            }
            Action::Continue
        }
        0x49 => {
            if session.block != 0 && cpu.selector(Segment::Extra) == session.block {
                session.block = 0;
                cpu.set_carry(false);
            } else {
                cpu.set_wide(Slot::Accumulator, 9);
                cpu.set_carry(true);
            }
            Action::Continue
        }
        0x4C => Action::Exit(cpu.low(Slot::Accumulator)),
        0x4E => {
            find_first(cpu, ram, session);
            Action::Continue
        }
        0x56 => {
            rename_path(cpu, ram);
            Action::Continue
        }
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
    let mut file = [0u8; 512];
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

pub(super) fn spawn(image: &[u8], ram: &mut GuestRam, session: &mut Session) -> u8 {
    let mut offset = 0u16;
    while offset < 0x0800 {
        if !ram.store(SHELL_CHILD, offset, 0) {
            return 0xFF;
        }
        offset = offset.wrapping_add(1);
    }
    let executable = image.first() == Some(&b'M') && image.get(1) == Some(&b'Z');
    let Some(mut cpu) = (if executable {
        super::mz::load(image, ram, SHELL_CHILD)
    } else {
        place_com(image, ram, SHELL_CHILD)
    }) else {
        return 0xFF;
    };
    match drive(&mut cpu, ram, session, 400) {
        End::Stopped(code) => code,
        _ => 0xFF,
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

fn named(cpu: &CpuState, ram: &GuestRam, act: fn(&[u8; 11]) -> bool) -> bool {
    let Some(name) = dos_name(&guest_text(cpu, ram)) else {
        return false;
    };
    act(&name)
}

fn change_directory(cpu: &CpuState, ram: &GuestRam) -> bool {
    let text = guest_text(cpu, ram);
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    super::disk::change_dir(&text[..end])
}

fn rename_path(cpu: &mut CpuState, ram: &GuestRam) {
    let Some(from) = dos_name(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    let Some(to) = dos_name(&bytes_at(
        ram,
        cpu.selector(Segment::Extra),
        cpu.wide(Slot::Dest),
    )) else {
        cpu.set_carry(true);
        return;
    };
    cpu.set_carry(!super::disk::rename_file(&from, &to));
}

fn seek_file(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle < 3 {
        cpu.set_carry(true);
        return;
    }
    let Some(Some(opened)) = session.opens.get_mut(usize::from(handle - 3)) else {
        cpu.set_carry(true);
        return;
    };
    let method = cpu.low(Slot::Accumulator);
    let offset = u32::from(cpu.wide(Slot::Count)) << 16 | u32::from(cpu.wide(Slot::Data));
    let next = match method {
        0 => offset,
        1 => (opened.pos as u32).wrapping_add(offset),
        2 => (opened.len as u32).wrapping_add(offset),
        _ => {
            cpu.set_carry(true);
            return;
        }
    };
    opened.pos = next as usize;
    cpu.set_wide(Slot::Accumulator, next as u16);
    cpu.set_wide(Slot::Data, (next >> 16) as u16);
    cpu.set_carry(false);
}

fn read_attribute(cpu: &mut CpuState, ram: &GuestRam) {
    if cpu.low(Slot::Accumulator) != 0 {
        cpu.set_carry(true);
        return;
    }
    let Some(name) = dos_name(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    let Some(attr) = volume::attribute(volume::IMAGE, &name) else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    cpu.set_wide(Slot::Count, u16::from(attr));
    cpu.set_carry(false);
}

fn write_root(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Source);
    let stored = ram.store(segment, offset, b'\\')
        && ram.store(segment, offset.wrapping_add(1), 0);
    cpu.set_carry(!stored);
}

fn find_first(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let Some(name) = dos_name(&guest_text(cpu, ram)) else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    if !volume::listed(volume::IMAGE, &name) || !write_found(ram, session, &name) {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    }
    cpu.set_wide(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn write_found(ram: &mut GuestRam, session: &Session, name: &[u8; 11]) -> bool {
    let mut shown = [0u8; 16];
    let mut length = 0usize;
    let mut index = 0usize;
    while index < 8 && name[index] != b' ' {
        shown[length] = name[index];
        length += 1;
        index += 1;
    }
    if name[8] != b' ' {
        shown[length] = b'.';
        length += 1;
        let mut ext = 8usize;
        while ext < 11 && name[ext] != b' ' {
            shown[length] = name[ext];
            length += 1;
            ext += 1;
        }
    }
    shown[length] = b'\r';
    length += 1;
    shown[length] = b'\n';
    length += 1;
    shown[length] = b'$';
    length += 1;
    let mut offset = session.dta_offset.wrapping_add(30);
    let mut cursor = 0usize;
    while cursor < length {
        if !ram.store(session.dta_segment, offset, shown[cursor]) {
            return false;
        }
        offset = offset.wrapping_add(1);
        cursor += 1;
    }
    true
}

fn report_space(cpu: &mut CpuState) {
    if cpu.low(Slot::Data) != 0 {
        cpu.set_wide(Slot::Accumulator, 0xFFFF);
        cpu.set_carry(true);
        return;
    }
    let (per_cluster, per_sector, clusters) = space();
    cpu.set_wide(Slot::Accumulator, per_cluster);
    cpu.set_wide(Slot::Base, clusters);
    cpu.set_wide(Slot::Count, per_sector);
    cpu.set_wide(Slot::Data, clusters);
    cpu.set_carry(false);
}

fn space() -> (u16, u16, u16) {
    let image = volume::IMAGE;
    let per_sector = u16::from_le_bytes([image[11], image[12]]);
    if per_sector == 0 {
        return (0, 0, 0);
    }
    let per_cluster = u16::from(image[13]);
    let reserved = u16::from_le_bytes([image[14], image[15]]);
    let fats = u16::from(image[16]);
    let root_entries = u16::from_le_bytes([image[17], image[18]]);
    let total = u16::from_le_bytes([image[19], image[20]]);
    let per_fat = u16::from_le_bytes([image[22], image[23]]);
    let root_bytes = u32::from(root_entries).saturating_mul(32);
    let root_sectors = root_bytes.div_ceil(u32::from(per_sector)) as u16;
    let data_at = reserved
        .saturating_add(fats.saturating_mul(per_fat))
        .saturating_add(root_sectors);
    (per_cluster, per_sector, total.saturating_sub(data_at))
}

fn bytes_at(ram: &GuestRam, segment: u16, mut offset: u16) -> [u8; 64] {
    let mut text = [0u8; 64];
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

fn serve_bios(cpu: &mut CpuState, ram: &mut GuestRam, vector: u8) -> bool {
    match vector {
        0x10 => video(cpu),
        0x13 => read_sectors(cpu, ram),
        0x16 => keyboard(cpu),
        0x1A => clock(cpu),
        _ => return false,
    }
    true
}

fn video(cpu: &mut CpuState) {
    match cpu.high(Slot::Accumulator) {
        0x0E => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Accumulator)]);
            cpu.set_carry(false);
        }
        0x0F => {
            cpu.set_high(Slot::Accumulator, 80);
            cpu.set_low(Slot::Accumulator, 3);
            cpu.set_high(Slot::Base, 0);
            cpu.set_carry(false);
        }
        _ => cpu.set_carry(true),
    }
}

fn keyboard(cpu: &mut CpuState) {
    if cpu.high(Slot::Accumulator) == 1 {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_zero(true);
        cpu.set_carry(false);
    } else {
        cpu.set_carry(true);
    }
}

fn clock(cpu: &mut CpuState) {
    if cpu.high(Slot::Accumulator) == 0 {
        cpu.set_wide(Slot::Count, 0);
        cpu.set_wide(Slot::Data, 1);
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_carry(true);
    }
}

fn read_sectors(cpu: &mut CpuState, ram: &mut GuestRam) {
    if cpu.high(Slot::Accumulator) != 2 || !copy_sectors(cpu, ram) {
        cpu.set_high(Slot::Accumulator, 1);
        cpu.set_carry(true);
    }
}

fn copy_sectors(cpu: &mut CpuState, ram: &mut GuestRam) -> bool {
    let image = volume::IMAGE;
    if image.len() < 30 {
        return false;
    }
    let per_track = u16::from_le_bytes([image[24], image[25]]);
    let heads = u16::from_le_bytes([image[26], image[27]]);
    let count = cpu.low(Slot::Accumulator);
    if per_track == 0 || heads == 0 || count == 0 || cpu.low(Slot::Data) != 0 {
        return false;
    }
    let packed = cpu.low(Slot::Count);
    let sector = packed & 0x3F;
    if sector == 0 {
        return false;
    }
    let cylinder = u32::from(cpu.high(Slot::Count)) | (u32::from(packed & 0xC0) << 2);
    let head = u32::from(cpu.high(Slot::Data));
    let linear = (cylinder * u32::from(heads) + head) * u32::from(per_track) + u32::from(sector - 1);
    let Some(start) = (linear as usize).checked_mul(512) else {
        return false;
    };
    let Some(bytes) = usize::from(count).checked_mul(512) else {
        return false;
    };
    let Some(end) = start.checked_add(bytes) else {
        return false;
    };
    let Some(data) = image.get(start..end) else {
        return false;
    };
    let segment = cpu.selector(Segment::Extra);
    let mut offset = cpu.wide(Slot::Base);
    for byte in data {
        if !ram.store(segment, offset, *byte) {
            return false;
        }
        offset = offset.wrapping_add(1);
    }
    cpu.set_low(Slot::Accumulator, count);
    cpu.set_high(Slot::Accumulator, 0);
    cpu.set_carry(false);
    true
}
