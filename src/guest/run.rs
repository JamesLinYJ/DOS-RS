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
const LAUNCH_SEGMENT: u16 = 0x1600;
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
    super::disk::boot();
    show_image(b"COMPRESSCOM", WIDE_SEGMENT, false, 20000);
    show_saved();
    crate::bios::mscon::con_write_bytes(b"EXE2BIN ");
    show_image(b"EXE2BIN EXE", EXE_PREFIX, true, 20000);
    show_exec();
    show_image(b"DBOF    EXE", EXE_PREFIX, true, 60000);
    show_image(b"EXEFIX  EXE", EXE_PREFIX, true, 60000);
    show_image(b"CONVERT EXE", EXE_PREFIX, true, 60000);
    show_tailed(b"ATTRIB  EXE", b" HELLO.COM", 60000);
    show_tailed(b"EXEMOD  EXE", b" HELLO.EXE /h", 60000);
}

fn show_tailed(name: &[u8; 11], tail: &[u8], limit: u16) {
    let mut image = [0u8; 16384];
    let Some(length) = volume::read_named(volume::IMAGE, name, &mut image) else {
        crate::bios::mscon::con_write_bytes(b"GUEST MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    let mut session = Session::new();
    session.prefix = EXE_PREFIX;
    let Some(mut cpu) = place(&image[..length], ram, EXE_PREFIX, true) else {
        crate::bios::mscon::con_write_bytes(b"GUEST FAULT\r\n");
        return;
    };
    if !ram.store(EXE_PREFIX, 0x80, tail.len() as u8) {
        return;
    }
    let mut index = 0u16;
    while usize::from(index) < tail.len() {
        if !ram.store(EXE_PREFIX, 0x81u16.wrapping_add(index), tail[usize::from(index)]) {
            return;
        }
        index = index.wrapping_add(1);
    }
    if !ram.store(EXE_PREFIX, 0x81u16.wrapping_add(index), b'\r') {
        return;
    }
    match drive(&mut cpu, ram, &mut session, u32::from(limit)) {
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

fn show_exec() {
    let mut image = [0u8; 16384];
    let Some(length) = volume::read_named(volume::IMAGE, b"EXEC    EXE", &mut image) else {
        crate::bios::mscon::con_write_bytes(b"EXEC MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    let mut session = Session::new();
    session.prefix = EXE_PREFIX;
    let Some(mut cpu) = place(&image[..length], ram, EXE_PREFIX, true) else {
        crate::bios::mscon::con_write_bytes(b"EXEC FAULT\r\n");
        return;
    };
    let tail = b" HELLO.COM";
    if !ram.store(EXE_PREFIX, 0x80, tail.len() as u8) {
        return;
    }
    let mut index = 0u16;
    while usize::from(index) < tail.len() {
        if !ram.store(EXE_PREFIX, 0x81u16.wrapping_add(index), tail[usize::from(index)]) {
            return;
        }
        index = index.wrapping_add(1);
    }
    if !ram.store(EXE_PREFIX, 0x81u16.wrapping_add(index), b'\r') {
        return;
    }
    match drive(&mut cpu, ram, &mut session, 60000) {
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

fn show_saved() {
    let mut body = [0u8; 32];
    let Some(length) = super::disk::read_file(b"SELECT  DAT", &mut body) else {
        crate::bios::mscon::con_write_bytes(b"COMPRESS MISS\r\n");
        return;
    };
    crate::bios::mscon::con_write_bytes(b"COMPRESS ");
    let mut index = 0usize;
    while index < length {
        put_hex8(body[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b"\r\n");
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
    session.prefix = SHELL_SEGMENT;
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
    report_header(b"PACKME  EXE");
}

fn report_header(name: &[u8; 11]) {
    let mut body = [0u8; 32];
    let Some(length) = super::disk::read_head(name, &mut body) else {
        return;
    };
    if length < 14 {
        return;
    }
    crate::bios::mscon::con_write_bytes(b"EXEFIX ");
    put_hex8(body[13]);
    put_hex8(body[12]);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(body[11]);
    put_hex8(body[10]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn show_image(name: &[u8; 11], segment: u16, executable: bool, limit: u16) {
    let mut image = [0u8; 16384];
    let Some(length) = volume::read_named(volume::IMAGE, name, &mut image) else {
        crate::bios::mscon::con_write_bytes(b"GUEST MISSING\r\n");
        return;
    };
    let ram = memory::storage();
    ram.clear();
    let mut session = Session::new();
    session.prefix = segment;
    let Some(mut cpu) = place(&image[..length], ram, segment, executable) else {
        crate::bios::mscon::con_write_bytes(b"GUEST FAULT\r\n");
        return;
    };
    match drive(&mut cpu, ram, &mut session, u32::from(limit)) {
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

fn drive(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session, limit: u32) -> End {
    let mut steps = 0u32;
    while steps < limit {
        steps += 1;
        match step::step(cpu, ram) {
            Outcome::Continue => {}
            Outcome::Interrupt(0x2E) => super::command::invoke(cpu, ram, session),
            Outcome::Interrupt(0x21) => match service(cpu, ram, session) {
                Action::Continue => {}
                Action::Exit(code) => return End::Stopped(code),
            },
            Outcome::Interrupt(0x03) => {}
            Outcome::Interrupt(0x20) | Outcome::Halt => return End::Stopped(0),
            Outcome::Fault(opcode) => return End::Fault(opcode),
            Outcome::Interrupt(vector) => {
                if serve_bios(cpu, ram, vector) {
                } else if !enter_vector(cpu, ram, session, vector) {
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

const FILE_BYTES: usize = 16384;

struct Opened {
    bytes: usize,
    len: usize,
    pos: usize,
    name: [u8; 11],
    named: bool,
}

static mut FILE_POOL: [[u8; FILE_BYTES]; 4] = [[0; FILE_BYTES]; 4];

fn file_bytes(index: usize) -> &'static mut [u8] {
    unsafe { &mut (*core::ptr::addr_of_mut!(FILE_POOL))[index] }
}

pub(super) struct Session {
    opens: [Option<Opened>; 4],
    last_code: u16,
    dta_segment: u16,
    dta_offset: u16,
    prefix: u16,
    block: u16,
    break_at: u16,
    vector_segment: [u16; 256],
    vector_offset: [u16; 256],
    vector_live: [bool; 256],
}

impl Session {
    fn new() -> Self {
        Self {
            opens: [None, None, None, None],
            last_code: 0,
            dta_segment: 0,
            dta_offset: 0,
            prefix: 0,
            block: 0,
            break_at: GUEST_BLOCK,
            vector_segment: [0; 256],
            vector_offset: [0; 256],
            vector_live: [false; 256],
        }
    }
}

fn service(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) -> Action {
    match cpu.high(Slot::Accumulator) {
        0x0B => {
            cpu.set_low(Slot::Accumulator, 0);
            cpu.set_carry(false);
            Action::Continue
        }
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
            write_output(cpu, ram, session);
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
            let id = usize::from(cpu.low(Slot::Accumulator));
            session.vector_segment[id] = cpu.selector(Segment::Data);
            session.vector_offset[id] = cpu.wide(Slot::Data);
            session.vector_live[id] = true;
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
            let id = usize::from(cpu.low(Slot::Accumulator));
            if session.vector_live[id] {
                cpu.set_selector(Segment::Extra, session.vector_segment[id]);
                cpu.set_wide(Slot::Base, session.vector_offset[id]);
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
            create_file(cpu, ram, session);
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
        0x44 => {
            device_control(cpu);
            Action::Continue
        }
        0x48 => {
            let asked = cpu.wide(Slot::Base);
            let room = super::memory::CONVENTIONAL_END.saturating_sub(session.break_at);
            if asked == 0 || asked > room {
                cpu.set_wide(Slot::Base, room);
                cpu.set_wide(Slot::Accumulator, 8);
                cpu.set_carry(true);
            } else {
                let given = session.break_at;
                session.break_at = session.break_at.wrapping_add(asked);
                session.block = given;
                cpu.set_wide(Slot::Accumulator, given);
                cpu.set_carry(false);
            }
            Action::Continue
        }
        0x49 => {
            let owned = cpu.selector(Segment::Extra);
            if owned >= GUEST_BLOCK && owned < session.break_at {
                cpu.set_carry(false);
            } else {
                cpu.set_wide(Slot::Accumulator, 9);
                cpu.set_carry(true);
            }
            Action::Continue
        }
        0x4A => {
            let block = cpu.selector(Segment::Extra);
            let asked = cpu.wide(Slot::Base);
            let room = super::memory::CONVENTIONAL_END.saturating_sub(block);
            if asked == 0 || asked > room {
                cpu.set_wide(Slot::Base, room);
                cpu.set_wide(Slot::Accumulator, 8);
                cpu.set_carry(true);
            } else {
                cpu.set_carry(false);
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
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    let mut file = [0u8; FILE_BYTES];
    let length = super::disk::read_file(&name, &mut file)
        .or_else(|| volume::read_named(volume::IMAGE, &name, &mut file));
    let Some(length) = length else {
        cpu.set_wide(Slot::Accumulator, 2);
        cpu.set_carry(true);
        return;
    };
    let mut slot = 0usize;
    while slot < session.opens.len() {
        if session.opens[slot].is_none() {
            let stored = file_bytes(slot);
            let count = length.min(stored.len());
            stored[..count].copy_from_slice(&file[..count]);
            session.opens[slot] = Some(Opened {
                bytes: slot,
                len: count,
                pos: 0,
                name,
                named: true,
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
        let byte = file_bytes(opened.bytes)[opened.pos];
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

fn create_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    if !super::disk::write_file(&name, b"") {
        cpu.set_carry(true);
        return;
    }
    let Some(slot) = free_handle(session) else {
        cpu.set_wide(Slot::Accumulator, 4);
        cpu.set_carry(true);
        return;
    };
    file_bytes(slot).fill(0);
    session.opens[slot] = Some(Opened {
        bytes: slot,
        len: 0,
        pos: 0,
        name,
        named: true,
    });
    cpu.set_wide(Slot::Accumulator, (slot as u16).wrapping_add(3));
    cpu.set_carry(false);
}

fn free_handle(session: &Session) -> Option<usize> {
    let mut slot = 0usize;
    while slot < session.opens.len() {
        if session.opens[slot].is_none() {
            return Some(slot);
        }
        slot += 1;
    }
    None
}

fn write_output(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle == 1 || handle == 2 {
        write_console(cpu, ram);
        return;
    }
    if handle < 3 {
        cpu.set_carry(true);
        return;
    }
    let Some(Some(opened)) = session.opens.get_mut(usize::from(handle - 3)) else {
        cpu.set_carry(true);
        return;
    };
    if !opened.named {
        cpu.set_carry(true);
        return;
    }
    let count = cpu.wide(Slot::Count);
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut seen = 0u16;
    while seen < count {
        if opened.pos >= FILE_BYTES {
            break;
        }
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        file_bytes(opened.bytes)[opened.pos] = byte;
        opened.pos += 1;
        if opened.pos > opened.len {
            opened.len = opened.pos;
        }
        offset = offset.wrapping_add(1);
        seen += 1;
    }
    let stored = super::disk::write_file(&opened.name, &file_bytes(opened.bytes)[..opened.len]);
    if stored {
        cpu.set_wide(Slot::Accumulator, seen);
        cpu.set_carry(false);
    } else {
        cpu.set_carry(true);
    }
}

fn write_console(cpu: &mut CpuState, ram: &GuestRam) {
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
        cpu.set_wide(Slot::Accumulator, 1);
        return;
    }
    let text = guest_text(cpu, ram);
    let block_segment = cpu.selector(Segment::Extra);
    let block_offset = cpu.wide(Slot::Base);
    let environment = environment_segment(ram, block_segment, block_offset);
    let Some(name) = resolve_program(&text, ram, environment) else {
        cpu.set_carry(true);
        cpu.set_wide(Slot::Accumulator, 2);
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
        cpu.set_wide(Slot::Accumulator, 8);
        return;
    };
    copy_command_tail(ram, block_segment, block_offset);
    match drive(&mut child, ram, session, 512) {
        End::Stopped(code) => {
            session.last_code = u16::from(code);
            cpu.set_wide(Slot::Accumulator, 0);
            cpu.set_carry(false);
        }
        _ => {
            cpu.set_wide(Slot::Accumulator, 1);
            cpu.set_carry(true);
        }
    }
}

fn environment_segment(ram: &GuestRam, segment: u16, offset: u16) -> u16 {
    let Some(low) = ram.load(segment, offset) else {
        return 0;
    };
    let Some(high) = ram.load(segment, offset.wrapping_add(1)) else {
        return 0;
    };
    let given = u16::from_le_bytes([low, high]);
    if given != 0 {
        return given;
    }
    let Some(parent) = ram.load(segment, 0x002C) else {
        return 0;
    };
    let Some(parent_high) = ram.load(segment, 0x002D) else {
        return 0;
    };
    u16::from_le_bytes([parent, parent_high])
}

fn resolve_program(text: &[u8; 64], ram: &GuestRam, environment: u16) -> Option<[u8; 11]> {
    if let Some(name) = file_component(text)
        && volume::listed(volume::IMAGE, &name)
    {
        return Some(name);
    }
    if directory_named(text) {
        return None;
    }
    let name = file_component(text)?;
    if path_lists(ram, environment, &name) {
        Some(name)
    } else {
        None
    }
}

fn directory_named(text: &[u8; 64]) -> bool {
    let mut index = 0usize;
    while index < text.len() && text[index] != 0 {
        if text[index] == b'\\' || text[index] == b'/' || text[index] == b':' {
            return true;
        }
        index += 1;
    }
    false
}

fn file_component(text: &[u8; 64]) -> Option<[u8; 11]> {
    let mut start = 0usize;
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        if text[end] == b'\\' || text[end] == b'/' || text[end] == b':' {
            start = end + 1;
        }
        end += 1;
    }
    if start >= end {
        return None;
    }
    let mut bare = [0u8; 64];
    let mut index = 0usize;
    while start < end && index + 1 < bare.len() {
        bare[index] = text[start];
        index += 1;
        start += 1;
    }
    dos_name(&bare)
}

fn path_lists(ram: &GuestRam, segment: u16, name: &[u8; 11]) -> bool {
    if segment == 0 || !volume::listed(volume::IMAGE, name) {
        return false;
    }
    let mut offset = 0u16;
    while offset < 0x1000 {
        if path_key(ram, segment, offset) {
            return path_value(ram, segment, offset.wrapping_add(5));
        }
        let Some(byte) = ram.load(segment, offset) else {
            return false;
        };
        if byte == 0 {
            return false;
        }
        while offset < 0x1000 {
            let Some(next) = ram.load(segment, offset) else {
                return false;
            };
            offset = offset.wrapping_add(1);
            if next == 0 {
                break;
            }
        }
    }
    false
}

fn path_key(ram: &GuestRam, segment: u16, offset: u16) -> bool {
    let key = b"PATH=";
    let mut index = 0usize;
    while index < key.len() {
        let Some(byte) = ram.load(segment, offset.wrapping_add(index as u16)) else {
            return false;
        };
        let folded = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            byte
        };
        if folded != key[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn path_value(ram: &GuestRam, segment: u16, mut offset: u16) -> bool {
    let mut used = false;
    while offset < 0x1000 {
        let Some(byte) = ram.load(segment, offset) else {
            return false;
        };
        if byte == 0 {
            return used;
        }
        if byte != b';' {
            used = true;
        }
        offset = offset.wrapping_add(1);
    }
    false
}

fn copy_command_tail(ram: &mut GuestRam, segment: u16, offset: u16) {
    let Some(tail_low) = ram.load(segment, offset.wrapping_add(2)) else {
        return;
    };
    let Some(tail_high) = ram.load(segment, offset.wrapping_add(3)) else {
        return;
    };
    let Some(tail_segment_low) = ram.load(segment, offset.wrapping_add(4)) else {
        return;
    };
    let Some(tail_segment_high) = ram.load(segment, offset.wrapping_add(5)) else {
        return;
    };
    let tail_offset = u16::from_le_bytes([tail_low, tail_high]);
    let tail_segment = u16::from_le_bytes([tail_segment_low, tail_segment_high]);
    let Some(count) = ram.load(tail_segment, tail_offset) else {
        return;
    };
    if !ram.store(LAUNCH_SEGMENT, 0x80, count) {
        return;
    }
    let mut index = 0u16;
    while index <= u16::from(count) && index < 0x7E {
        let Some(byte) = ram.load(tail_segment, tail_offset.wrapping_add(1).wrapping_add(index))
        else {
            return;
        };
        if !ram.store(LAUNCH_SEGMENT, 0x81u16.wrapping_add(index), byte) {
            return;
        }
        index = index.wrapping_add(1);
    }
}

fn place_child(image: &[u8], ram: &mut GuestRam, executable: bool) -> Option<CpuState> {
    if executable {
        super::mz::load(image, ram, LAUNCH_SEGMENT)
    } else {
        place_com(image, ram, LAUNCH_SEGMENT)
    }
}

pub(super) fn spawn(
    image: &[u8],
    args: &[u8],
    ram: &mut GuestRam,
    session: &mut Session,
) -> u8 {
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
    if !args.is_empty() {
        let prefix = SHELL_CHILD;
        let count = args.len().min(126);
        if !ram.store(prefix, 0x80, (count + 1) as u8) {
            return 0xFF;
        }
        if !ram.store(prefix, 0x81, b' ') {
            return 0xFF;
        }
        let mut index = 0usize;
        while index < count {
            if !ram.store(prefix, 0x82u16.wrapping_add(index as u16), args[index]) {
                return 0xFF;
            }
            index += 1;
        }
        if !ram.store(prefix, 0x82u16.wrapping_add(index as u16), b'\r') {
            return 0xFF;
        }
    }
    match drive(&mut cpu, ram, session, 60000) {
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
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
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
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
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

fn dta_target(session: &Session) -> (u16, u16) {
    if session.dta_segment == 0 && session.dta_offset == 0 && session.prefix != 0 {
        (session.prefix, 0x80)
    } else {
        (session.dta_segment, session.dta_offset)
    }
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
    shown[length] = 0;
    length += 1;
    let (segment, base) = dta_target(session);
    let attr = volume::attribute(volume::IMAGE, name).unwrap_or(0x20);
    if !ram.store(segment, base.wrapping_add(0x15), attr) {
        return false;
    }
    let mut offset = base.wrapping_add(30);
    let mut cursor = 0usize;
    while cursor < length {
        if !ram.store(segment, offset, shown[cursor]) {
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

fn device_control(cpu: &mut CpuState) {
    if cpu.low(Slot::Accumulator) != 0 {
        cpu.set_carry(true);
        return;
    }
    let info = if cpu.wide(Slot::Base) < 3 { 0x80D3 } else { 0 };
    cpu.set_wide(Slot::Data, info);
    cpu.set_carry(false);
}

fn enter_vector(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session, vector: u8) -> bool {
    let id = usize::from(vector);
    if !session.vector_live[id] {
        return false;
    }
    let flags = cpu.flags();
    let caller = cpu.selector(Segment::Code);
    let next = cpu.instruction();
    if !guest_push(cpu, ram, flags) || !guest_push(cpu, ram, caller) || !guest_push(cpu, ram, next)
    {
        return false;
    }
    cpu.set_selector(Segment::Code, session.vector_segment[id]);
    cpu.set_instruction(session.vector_offset[id]);
    true
}

fn guest_push(cpu: &mut CpuState, ram: &mut GuestRam, value: u16) -> bool {
    let pointer = cpu.wide(Slot::Stack).wrapping_sub(2);
    cpu.set_wide(Slot::Stack, pointer);
    let bytes = value.to_le_bytes();
    let segment = cpu.selector(Segment::Stack);
    ram.store(segment, pointer, bytes[0]) && ram.store(segment, pointer.wrapping_add(1), bytes[1])
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
