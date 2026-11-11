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
    session.current_psp = EXE_PREFIX;
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
    session.current_psp = EXE_PREFIX;
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
    session.current_psp = SHELL_SEGMENT;
    let Some(mut cpu) = place(&image[..length], ram, SHELL_SEGMENT, false) else {
        crate::bios::mscon::con_write_bytes(b"CMD FAULT\r\n");
        return;
    };
    match drive(&mut cpu, ram, &mut session, 100_000) {
        End::Stopped(_) => {}
        End::Fault(opcode) => {
            crate::bios::mscon::con_write_bytes(b"CMD FAULT ");
            put_hex8(opcode);
            crate::bios::mscon::con_write_bytes(b"\r\n");
        }
        End::Other => crate::bios::mscon::con_write_bytes(b"CMD INT\r\n"),
        End::Loop => crate::bios::mscon::con_write_bytes(b"CMD LOOP\r\n"),
    }
    if let Some(size) = super::disk::file_len(b"SAY     BIN") {
        let mut body = [0u8; 32];
        let length = super::disk::read_head(b"SAY     BIN", &mut body).unwrap_or(0);
        crate::bios::mscon::con_write_bytes(b"EXE2BIN ");
        put_hex8((size >> 8) as u8);
        put_hex8(size as u8);
        if length >= 2 && size == 0x18 && body[0] == 0x0E && body[1] == 0x1F {
            crate::bios::mscon::con_write_bytes(b" 0E1F OK\r\n");
        } else {
            crate::bios::mscon::con_write_bytes(b" NO\r\n");
        }
    }
    report_header(b"EXEFIX ", b"PACKME  EXE");
    report_header(b"PACKFIX ", b"FIX2    EXE");
    report_packed();
    report_listing();
}

fn report_listing() {
    let mut body = [0u8; 256];
    let Some(length) = super::disk::read_head(b"LIB     LST", &mut body) else {
        return;
    };
    crate::bios::mscon::con_write_bytes(b"LIB LST ");
    let mut index = 0usize;
    let mut shown = 0usize;
    while index < length && shown < 48 {
        let byte = body[index];
        if byte == b'\r' || byte == b'\n' {
            break;
        }
        if (0x20..0x7F).contains(&byte) {
            crate::bios::mscon::con_write_bytes(&[byte]);
            shown += 1;
        }
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn report_packed() {
    let Some(size) = super::disk::file_len(b"PACKED  EXE") else {
        return;
    };
    let mut body = [0u8; 16384];
    let Some(length) = super::disk::read_head(b"PACKED  EXE", &mut body) else {
        return;
    };
    let mark = b"Packed file is corrupt";
    let mut found = false;
    let mut index = 0usize;
    while index + mark.len() <= length {
        if &body[index..index + mark.len()] == mark {
            found = true;
            break;
        }
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b"PACKED ");
    put_hex8((size >> 8) as u8);
    put_hex8(size as u8);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(body[0]);
    put_hex8(body[1]);
    if found {
        crate::bios::mscon::con_write_bytes(b" OK\r\n");
    } else {
        crate::bios::mscon::con_write_bytes(b" NO\r\n");
    }
}

fn report_header(label: &[u8], name: &[u8; 11]) {
    let mut body = [0u8; 32];
    let Some(length) = super::disk::read_head(name, &mut body) else {
        return;
    };
    if length < 14 {
        return;
    }
    crate::bios::mscon::con_write_bytes(label);
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
    session.current_psp = segment;
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

const FILE_BYTES: usize = 262144;

#[derive(Clone, Copy)]
struct Opened {
    bytes: usize,
    len: usize,
    pos: usize,
    name: [u8; 11],
    named: bool,
    console: bool,
    access: u8,
    dirty: bool,
}

const HANDLE_SLOTS: usize = 20;

static mut FILE_POOL: [[u8; FILE_BYTES]; HANDLE_SLOTS] = [[0; FILE_BYTES]; HANDLE_SLOTS];

fn file_bytes(index: usize) -> &'static mut [u8] {
    unsafe { &mut (*core::ptr::addr_of_mut!(FILE_POOL))[index] }
}

pub(super) struct Session {
    opens: [Option<Opened>; HANDLE_SLOTS],
    last_code: u16,
    dta_segment: u16,
    dta_offset: u16,
    prefix: u16,
    block: u16,
    break_at: u16,
    drive: u8,
    break_check: u8,
    extended_break: u8,
    current_psp: u16,
    extended: u16,
    extended_class: u8,
    extended_action: u8,
    extended_locus: u8,
    stdio: [Option<Opened>; 3],
    /// First paragraph past the bytes of the program now running.
    image_limit: u16,
    /// Paragraph where that program asked to end its own memory block.
    kept_end: u16,
    vector_segment: [u16; 256],
    vector_offset: [u16; 256],
    vector_live: [bool; 256],
    verify: u8,
}

impl Session {
    fn new() -> Self {
        Self {
            opens: [None; HANDLE_SLOTS],
            last_code: 0,
            dta_segment: 0,
            dta_offset: 0,
            prefix: 0,
            block: 0,
            break_at: GUEST_BLOCK,
            drive: 0,
            break_check: 0,
            extended_break: 0,
            current_psp: 0,
            extended: 0,
            extended_class: 0,
            extended_action: 0,
            extended_locus: 0,
            stdio: [None, None, None],
            image_limit: 0,
            kept_end: 0,
            vector_segment: [0; 256],
            vector_offset: [0; 256],
            vector_live: [false; 256],
            verify: 0,
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
            cpu.set_low(Slot::Accumulator, session.drive);
            cpu.set_carry(false);
            Action::Continue
        }
        0x0E => {
            let requested = cpu.low(Slot::Data);
            if requested < 2 {
                session.drive = requested;
            }
            cpu.set_low(Slot::Accumulator, 2);
            cpu.set_carry(false);
            Action::Continue
        }
        0x29 => {
            parse_filename(cpu, ram);
            Action::Continue
        }
        0x33 => {
            system_values(cpu, session);
            Action::Continue
        }
        0x34 => {
            indos_flag(cpu, ram);
            Action::Continue
        }
        0x46 => {
            force_duplicate(cpu, session);
            Action::Continue
        }
        0x59 => {
            extended_error(cpu, session);
            Action::Continue
        }
        0x62 => {
            current_process(cpu, session);
            Action::Continue
        }
        0x1B => {
            cpu.set_low(Slot::Data, 0);
            drive_info(cpu, ram);
            Action::Continue
        }
        0x1C => {
            drive_info(cpu, ram);
            Action::Continue
        }
        0x1A => {
            session.dta_segment = cpu.selector(Segment::Data);
            session.dta_offset = cpu.wide(Slot::Data);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2F => {
            let (segment, offset) = dta_target(session);
            cpu.set_selector(Segment::Extra, segment);
            cpu.set_wide(Slot::Base, offset);
            cpu.set_carry(false);
            Action::Continue
        }
        0x0F => {
            fcb_open(cpu, ram);
            Action::Continue
        }
        0x13 => {
            fcb_delete(cpu, ram);
            Action::Continue
        }
        0x14 => {
            fcb_read(cpu, ram, session);
            Action::Continue
        }
        0x15 => {
            fcb_write(cpu, ram, session);
            Action::Continue
        }
        0x16 => {
            fcb_create(cpu, ram);
            Action::Continue
        }
        0x17 => {
            fcb_rename(cpu, ram);
            Action::Continue
        }
        0x21 => {
            fcb_random_read(cpu, ram, session);
            Action::Continue
        }
        0x22 => {
            fcb_random_write(cpu, ram, session);
            Action::Continue
        }
        0x23 => {
            fcb_size(cpu, ram);
            Action::Continue
        }
        0x24 => {
            fcb_place(cpu, ram);
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
        0x2E => {
            session.verify = u8::from(cpu.low(Slot::Accumulator) != 0);
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
            } else {
                cpu.set_selector(Segment::Extra, 0);
                cpu.set_wide(Slot::Base, 0);
            }
            cpu.set_carry(false);
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
            current_directory(cpu, ram, session);
            Action::Continue
        }
        0x44 => {
            device_control(cpu, session);
            Action::Continue
        }
        0x63 => {
            if cpu.low(Slot::Accumulator) == 0
                && ram.store(SHELL_CHILD, 0xFE, 0)
                && ram.store(SHELL_CHILD, 0xFF, 0)
            {
                cpu.set_selector(Segment::Data, SHELL_CHILD);
                cpu.set_wide(Slot::Source, 0xFE);
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
            Action::Continue
        }
        0x45 => {
            duplicate(cpu, session);
            Action::Continue
        }
        0x48 => {
            allocate(cpu, ram, session);
            Action::Continue
        }
        0x49 => {
            let owned = cpu.selector(Segment::Extra);
            let low = owned >= GUEST_BLOCK && owned < session.break_at;
            let kept = session.kept_end > session.image_limit
                && owned >= session.image_limit
                && owned < session.kept_end;
            if low || kept || owned == session.block {
                cpu.set_carry(false);
            } else {
                fail(cpu, session, 9);
            }
            Action::Continue
        }
        0x4A => {
            resize_block(cpu, session);
            Action::Continue
        }
        0x4C => Action::Exit(cpu.low(Slot::Accumulator)),
        0x4E => {
            find_first(cpu, ram, session);
            Action::Continue
        }
        0x4F => {
            find_next(cpu, ram, session);
            Action::Continue
        }
        0x54 => {
            cpu.set_low(Slot::Accumulator, session.verify);
            cpu.set_carry(false);
            Action::Continue
        }
        0x56 => {
            rename_path(cpu, ram);
            Action::Continue
        }
        0x57 => {
            file_time(cpu, session);
            Action::Continue
        }
        0x5A => {
            create_temp(cpu, ram, session);
            Action::Continue
        }
        0x5B => {
            create_new(cpu, ram, session);
            Action::Continue
        }
        0x6C => {
            extended_open(cpu, ram, session);
            Action::Continue
        }
        0x60 => {
            truename(cpu, ram, session);
            Action::Continue
        }
        0x68 => {
            commit_file(cpu, session);
            Action::Continue
        }
        0x4D => {
            cpu.set_wide(Slot::Accumulator, session.last_code);
            session.last_code = 0;
            cpu.set_carry(false);
            Action::Continue
        }
        _ => {
            fail(cpu, session, 1);
            Action::Continue
        }
    }
}

const ERROR_MAP: [(u16, u8, u8, u8); 31] = [
    (1, 7, 4, 0xFF),
    (2, 8, 3, 2),
    (3, 8, 3, 2),
    (4, 1, 4, 1),
    (5, 3, 3, 0xFF),
    (6, 7, 4, 1),
    (7, 7, 5, 5),
    (8, 1, 4, 5),
    (9, 7, 4, 5),
    (10, 7, 4, 5),
    (11, 9, 3, 1),
    (12, 7, 4, 1),
    (13, 9, 4, 1),
    (15, 8, 3, 2),
    (16, 3, 3, 2),
    (17, 13, 3, 2),
    (18, 8, 3, 2),
    (32, 10, 2, 2),
    (33, 10, 2, 2),
    (36, 1, 4, 5),
    (38, 1, 4, 1),
    (39, 1, 4, 1),
    (50, 9, 3, 3),
    (80, 12, 3, 2),
    (82, 1, 4, 2),
    (83, 13, 4, 1),
    (84, 1, 4, 0xFF),
    (85, 12, 3, 3),
    (86, 3, 3, 1),
    (87, 9, 3, 1),
    (90, 13, 4, 2),
];

fn note_error(session: &mut Session, code: u16) {
    session.extended = code;
    for (number, class, action, locus) in ERROR_MAP {
        if number == code {
            session.extended_class = class;
            session.extended_action = action;
            if locus != 0xFF {
                session.extended_locus = locus;
            }
            return;
        }
    }
}

fn fail(cpu: &mut CpuState, session: &mut Session, code: u16) {
    note_error(session, code);
    cpu.set_wide(Slot::Accumulator, code);
    cpu.set_carry(true);
}

fn extended_error(cpu: &mut CpuState, session: &Session) {
    cpu.set_wide(Slot::Accumulator, session.extended);
    cpu.set_wide(
        Slot::Base,
        (u16::from(session.extended_class) << 8) | u16::from(session.extended_action),
    );
    let kept = cpu.wide(Slot::Count) & 0x00FF;
    cpu.set_wide(
        Slot::Count,
        kept | (u16::from(session.extended_locus) << 8),
    );
    cpu.set_selector(Segment::Extra, 0);
    cpu.set_wide(Slot::Dest, 0);
    cpu.set_carry(false);
}

fn current_process(cpu: &mut CpuState, session: &Session) {
    cpu.set_wide(Slot::Base, session.current_psp);
    cpu.set_carry(false);
}

fn console_handle(handle: u16) -> Opened {
    Opened {
        bytes: 0,
        len: 0,
        pos: 0,
        name: [b' '; 11],
        named: false,
        console: true,
        access: if handle == 0 { 0 } else { 1 },
        dirty: false,
    }
}

fn copy_handle(session: &Session, handle: u16) -> Option<Opened> {
    if handle < 3 {
        if let Some(opened) = session.stdio[usize::from(handle)] {
            return Some(opened);
        }
        return Some(console_handle(handle));
    }
    session
        .opens
        .get(usize::from(handle - 3))
        .copied()
        .flatten()
}

fn force_duplicate(cpu: &mut CpuState, session: &mut Session) {
    let source = cpu.wide(Slot::Base);
    let target = cpu.wide(Slot::Count);
    let Some(opened) = copy_handle(session, source) else {
        fail(cpu, session, 6);
        return;
    };
    if target >= (HANDLE_SLOTS as u16).saturating_add(3) {
        fail(cpu, session, 6);
        return;
    }
    if source == target {
        cpu.set_carry(false);
        return;
    }
    if target < 3 {
        session.stdio[usize::from(target)] = Some(opened);
        cpu.set_carry(false);
        return;
    }
    let index = usize::from(target - 3);
    if let Some(previous) = session.opens[index] {
        let _ = flush_opened(&previous);
    }
    session.opens[index] = Some(opened);
    cpu.set_carry(false);
}

fn flush_stdio(session: &Session) {
    let mut index = 0usize;
    while index < session.stdio.len() {
        if let Some(opened) = session.stdio[index] {
            let _ = flush_opened(&opened);
        }
        index += 1;
    }
}

fn open_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let access = cpu.low(Slot::Accumulator) & 7;
    if access > 2 {
        fail(cpu, session, 12);
        return;
    }
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
        fail(cpu, session, 2);
        return;
    };
    let Some(slot) = free_handle(session) else {
        fail(cpu, session, 4);
        return;
    };
    let stored = file_bytes(slot);
    let length = super::disk::read_file(&name, stored)
        .or_else(|| volume::read_named(volume::IMAGE, &name, stored));
    let Some(length) = length else {
        fail(cpu, session, 2);
        return;
    };
    session.opens[slot] = Some(Opened {
        bytes: slot,
        len: length,
        pos: 0,
        name,
        named: true,
        console: false,
        access,
        dirty: false,
    });
    cpu.set_wide(Slot::Accumulator, (slot as u16).wrapping_add(3));
    cpu.set_carry(false);
}

fn flush_opened(opened: &Opened) -> bool {
    if !opened.dirty || !opened.named || opened.console {
        return true;
    }
    super::disk::write_file(&opened.name, &file_bytes(opened.bytes)[..opened.len])
}

fn commit_file(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    let result = if handle < 3 {
        let index = usize::from(handle);
        if let Some(opened) = session.stdio[index].as_mut() {
            if flush_opened(opened) {
                opened.dirty = false;
                Commit::Done
            } else {
                Commit::Failed
            }
        } else {
            Commit::Done
        }
    } else {
        let index = usize::from(handle - 3);
        if let Some(opened) = session.opens.get_mut(index).and_then(|slot| slot.as_mut()) {
            if flush_opened(opened) {
                opened.dirty = false;
                Commit::Done
            } else {
                Commit::Failed
            }
        } else {
            Commit::Missing
        }
    };
    match result {
        Commit::Done => cpu.set_carry(false),
        Commit::Missing => fail(cpu, session, 6),
        Commit::Failed => fail(cpu, session, 5),
    }
}

enum Commit {
    Done,
    Missing,
    Failed,
}

fn close_file(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle >= 3 {
        let index = usize::from(handle - 3);
        if let Some(opened) = session.opens[index] {
            let stored = flush_opened(&opened);
            session.opens[index] = None;
            if !stored {
                note_error(session, 5);
            }
            cpu.set_carry(!stored);
            return;
        }
    }
    note_error(session, 6);
    cpu.set_carry(true);
}

fn read_file(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    let requested = usize::from(cpu.wide(Slot::Count));
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
    if handle < 3 {
        if session.stdio[usize::from(handle)].is_some() {
            read_stdio(cpu, ram, session, handle);
            return;
        }
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
        return;
    }
    let Some(found) = session
        .opens
        .get(usize::from(handle - 3))
        .copied()
        .flatten()
    else {
        fail(cpu, session, 6);
        return;
    };
    if found.access == 1 {
        fail(cpu, session, 5);
        return;
    }
    let Some(Some(opened)) = session.opens.get_mut(usize::from(handle - 3)) else {
        fail(cpu, session, 6);
        return;
    };
    let mut limit = requested;
    if &opened.name[8..11] == b"SKL" {
        let mut index = opened.pos;
        let end = opened.len.min(opened.pos.saturating_add(requested));
        while index < end {
            index += 1;
            if file_bytes(opened.bytes)[index - 1] == b'\n' {
                break;
            }
        }
        limit = index - opened.pos;
    }
    let mut moved = 0u16;
    while usize::from(moved) < limit && opened.pos < opened.len {
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

fn create_temp(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let segment = cpu.selector(Segment::Data);
    let start = cpu.wide(Slot::Data);
    let text = bytes_at(ram, segment, start);
    let mut length = 0usize;
    while length < text.len() && text[length] != 0 {
        length += 1;
    }
    if length == 0 || length == text.len() {
        fail(cpu, session, 3);
        return;
    }
    let saved = super::disk::working_directory();
    if let Err(code) = temp_directory(&text[..length]) {
        super::disk::set_working(saved);
        fail(cpu, session, code);
        return;
    }
    let attr = (cpu.wide(Slot::Count) as u8) & 0x27;
    let mut serial = 0u32;
    let mut name = [b' '; 11];
    let chosen = loop {
        if serial > 0xFFFF {
            super::disk::set_working(saved);
            fail(cpu, session, 4);
            return;
        }
        hex_name(serial, &mut name);
        if !super::disk::exists(&name) && !volume::listed(volume::IMAGE, &name) {
            break name;
        }
        serial += 1;
    };
    if !super::disk::write_named(&chosen, b"", attr) {
        super::disk::set_working(saved);
        fail(cpu, session, 5);
        return;
    }
    let Some(handle) = open_named(session, chosen, 2, true) else {
        super::disk::set_working(saved);
        fail(cpu, session, 4);
        return;
    };
    super::disk::set_working(saved);
    let mut index = 0usize;
    let mut offset = start.wrapping_add(length as u16);
    while index < 8 {
        if !ram.store(segment, offset, chosen[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if !ram.store(segment, offset, 0) {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_wide(Slot::Accumulator, handle);
    cpu.set_carry(false);
}

fn temp_directory(text: &[u8]) -> Result<(), u16> {
    let mut cursor = 0usize;
    while cursor < text.len() && text[cursor] == b' ' {
        cursor += 1;
    }
    if cursor >= text.len() {
        return Err(3);
    }
    if cursor + 1 < text.len() && text[cursor + 1] == b':' {
        let letter = ascii_upper(text[cursor]);
        if !letter.is_ascii_uppercase() || letter > b'B' {
            return Err(15);
        }
        cursor += 2;
    }
    let last = text[text.len() - 1];
    if cursor >= text.len() || (last != b'\\' && last != b'/') {
        return Err(3);
    }
    let body = &text[cursor..text.len() - 1];
    if body.is_empty() {
        super::disk::change_dir(b"\\");
        return Ok(());
    }
    if body.contains(&b'\\') || body.contains(&b'/') || !super::disk::change_dir(body) {
        return Err(3);
    }
    Ok(())
}

fn hex_name(serial: u32, name: &mut [u8; 11]) {
    let digits = b"0123456789ABCDEF";
    let mut index = 0usize;
    while index < 8 {
        let shift = (7 - index) * 4;
        name[index] = digits[((serial >> shift) & 0xF) as usize];
        index += 1;
    }
    name[8] = b' ';
    name[9] = b' ';
    name[10] = b' ';
}

fn create_new(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    if let Some(name) = file_component(&guest_text(cpu, ram))
        && super::disk::exists(&name)
    {
        fail(cpu, session, 0x50);
        return;
    }
    create_file(cpu, ram, session);
}

fn extended_open(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        fail(cpu, session, 1);
        return;
    }
    let action = cpu.wide(Slot::Data);
    let access = cpu.low(Slot::Base) & 7;
    if access > 2 {
        fail(cpu, session, 12);
        return;
    }
    let attr = (cpu.wide(Slot::Count) as u8) & 0x27;
    let Some(name) = file_component(&bytes_at(
        ram,
        cpu.selector(Segment::Data),
        cpu.wide(Slot::Source),
    )) else {
        fail(cpu, session, 2);
        return;
    };
    let on_disk = super::disk::exists(&name);
    let present = on_disk || volume::listed(volume::IMAGE, &name);
    let exist_action = action & 0x0F;
    let missing_action = action >> 4;
    let (empty, taken) = if present && exist_action == 1 {
        (false, 1u16)
    } else if present && exist_action == 2 && on_disk {
        if !super::disk::write_named(&name, b"", attr) {
            fail(cpu, session, 5);
            return;
        }
        (true, 3u16)
    } else if present {
        fail(cpu, session, 0x50);
        return;
    } else if missing_action == 1 {
        if !super::disk::write_named(&name, b"", attr) {
            fail(cpu, session, 5);
            return;
        }
        (true, 2u16)
    } else {
        fail(cpu, session, 2);
        return;
    };
    let Some(handle) = open_named(session, name, access, empty) else {
        fail(cpu, session, 4);
        return;
    };
    cpu.set_wide(Slot::Accumulator, handle);
    cpu.set_wide(Slot::Count, taken);
    cpu.set_carry(false);
}

fn open_named(session: &mut Session, name: [u8; 11], access: u8, empty: bool) -> Option<u16> {
    let slot = free_handle(session)?;
    let stored = file_bytes(slot);
    let length = if empty {
        stored.fill(0);
        0
    } else {
        super::disk::read_file(&name, stored)
            .or_else(|| volume::read_named(volume::IMAGE, &name, stored))?
    };
    session.opens[slot] = Some(Opened {
        bytes: slot,
        len: length,
        pos: 0,
        name,
        named: true,
        console: false,
        access,
        dirty: false,
    });
    Some((slot as u16).wrapping_add(3))
}

fn create_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
        fail(cpu, session, 2);
        return;
    };
    let attr = (cpu.wide(Slot::Count) as u8) & 0x27;
    if !super::disk::write_named(&name, b"", attr) {
        fail(cpu, session, 0x27);
        return;
    }
    let Some(slot) = free_handle(session) else {
        fail(cpu, session, 4);
        return;
    };
    file_bytes(slot).fill(0);
    session.opens[slot] = Some(Opened {
        bytes: slot,
        len: 0,
        pos: 0,
        name,
        named: true,
        console: false,
        access: 2,
        dirty: false,
    });
    cpu.set_wide(Slot::Accumulator, (slot as u16).wrapping_add(3));
    cpu.set_carry(false);
}

fn duplicate(cpu: &mut CpuState, session: &mut Session) {
    let source = cpu.wide(Slot::Base);
    let Some(slot) = free_handle(session) else {
        cpu.set_wide(Slot::Accumulator, 4);
        cpu.set_carry(true);
        return;
    };
    let opened = if source < 3 {
        Opened {
            bytes: 0,
            len: 0,
            pos: 0,
            name: [b' '; 11],
            named: false,
            console: true,
            access: if source == 0 { 0 } else { 1 },
            dirty: false,
        }
    } else {
        let Some(found) = session
            .opens
            .get(usize::from(source - 3))
            .copied()
            .flatten()
        else {
            cpu.set_wide(Slot::Accumulator, 6);
            cpu.set_carry(true);
            return;
        };
        found
    };
    session.opens[slot] = Some(opened);
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
    if handle < 3 && session.stdio[usize::from(handle)].is_some() {
        write_stdio(cpu, ram, session, handle);
        return;
    }
    if handle == 1 || handle == 2 {
        write_console(cpu, ram);
        return;
    }
    if handle < 3 {
        fail(cpu, session, 6);
        return;
    }
    let Some(found) = session
        .opens
        .get(usize::from(handle - 3))
        .copied()
        .flatten()
    else {
        fail(cpu, session, 6);
        return;
    };
    if found.console {
        write_console(cpu, ram);
        return;
    }
    if !found.named || found.access == 0 {
        fail(cpu, session, 5);
        return;
    }
    let Some(Some(opened)) = session.opens.get_mut(usize::from(handle - 3)) else {
        fail(cpu, session, 6);
        return;
    };
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
    if count == 0 {
        opened.len = opened.pos;
    }
    opened.dirty = true;
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

fn read_stdio(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session, handle: u16) {
    let Some(mut opened) = session.stdio[usize::from(handle)] else {
        return;
    };
    if opened.access == 1 {
        fail(cpu, session, 5);
        return;
    }
    let requested = usize::from(cpu.wide(Slot::Count));
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
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
    session.stdio[usize::from(handle)] = Some(opened);
    cpu.set_wide(Slot::Accumulator, moved);
    cpu.set_carry(false);
}

fn write_stdio(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session, handle: u16) {
    let Some(mut opened) = session.stdio[usize::from(handle)] else {
        return;
    };
    if opened.console {
        write_console(cpu, ram);
        return;
    }
    if !opened.named || opened.access == 0 {
        fail(cpu, session, 5);
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
    if count == 0 {
        opened.len = opened.pos;
    }
    opened.dirty = true;
    session.stdio[usize::from(handle)] = Some(opened);
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

fn write_console(cpu: &mut CpuState, ram: &GuestRam) {
    let count = cpu.wide(Slot::Count);
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut seen = 0u16;
    while seen < count {
        let mut text = [0u8; 64];
        let mut length = 0usize;
        while seen < count && length < text.len() {
            let Some(byte) = ram.load(segment, offset) else {
                break;
            };
            text[length] = byte;
            length += 1;
            seen += 1;
            offset = offset.wrapping_add(1);
        }
        if length == 0 {
            break;
        }
        crate::bios::mscon::con_write_bytes(&text[..length]);
    }
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

const NESTED_BYTES: usize = 262144;
static mut NESTED: [u8; NESTED_BYTES] = [0; NESTED_BYTES];

fn nested_image() -> &'static mut [u8; NESTED_BYTES] {
    unsafe { &mut *core::ptr::addr_of_mut!(NESTED) }
}

fn launch(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        fail(cpu, session, 1);
        return;
    }
    let text = guest_text(cpu, ram);
    let block_segment = cpu.selector(Segment::Extra);
    let block_offset = cpu.wide(Slot::Base);
    let environment = environment_segment(ram, block_segment, block_offset);
    let Some(name) = resolve_program(&text, ram, environment) else {
        fail(cpu, session, 2);
        return;
    };
    let image = nested_image();
    let Some(length) = volume::read_named(volume::IMAGE, &name, image) else {
        fail(cpu, session, 2);
        return;
    };
    let executable = image.first() == Some(&b'M') && image.get(1) == Some(&b'Z');
    let segment = if session.break_at > LAUNCH_SEGMENT {
        session.break_at
    } else {
        LAUNCH_SEGMENT
    };
    let Some(mut child) = place_child(&image[..length], ram, executable, segment) else {
        fail(cpu, session, 8);
        return;
    };
    let arena = session.break_at;
    let saved_limit = session.image_limit;
    let saved_kept = session.kept_end;
    note_image(session, &image[..length], segment.saturating_add(0x0010));
    if length > 32 * 1024 {
        let load_segment = segment.saturating_add(0x0010);
        let image_end = resident_end(&image[..length], load_segment);
        if image_end > session.break_at && image_end < super::memory::CONVENTIONAL_END {
            session.break_at = image_end;
        }
    }
    let given_environment = u16::from_le_bytes([
        ram.load(block_segment, block_offset).unwrap_or(0),
        ram.load(block_segment, block_offset.wrapping_add(1)).unwrap_or(0),
    ]);
    if given_environment != 0 {
        let bytes = given_environment.to_le_bytes();
        let _ = ram.store(segment, 0x2C, bytes[0]);
        let _ = ram.store(segment, 0x2D, bytes[1]);
    }
    copy_command_tail(ram, block_segment, block_offset, segment);
    let mut inherited = [false; HANDLE_SLOTS];
    let mut slot = 0usize;
    while slot < HANDLE_SLOTS {
        inherited[slot] = session.opens[slot].is_some();
        slot += 1;
    }
    let saved_psp = session.current_psp;
    let saved_stdio = session.stdio;
    session.current_psp = segment;
    match drive(&mut child, ram, session, 8_000_000) {
        End::Stopped(code) => {
            session.last_code = u16::from(code);
            cpu.set_wide(Slot::Accumulator, 0);
            cpu.set_carry(false);
        }
        _ => {
            note_error(session, 1);
            cpu.set_wide(Slot::Accumulator, 1);
            cpu.set_carry(true);
        }
    }
    slot = 0;
    while slot < HANDLE_SLOTS {
        if !inherited[slot] {
            if let Some(opened) = session.opens[slot] {
                let _ = flush_opened(&opened);
            }
            session.opens[slot] = None;
        }
        slot += 1;
    }
    flush_stdio(session);
    session.current_psp = saved_psp;
    session.stdio = saved_stdio;
    session.break_at = arena;
    session.image_limit = saved_limit;
    session.kept_end = saved_kept;
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

fn copy_command_tail(ram: &mut GuestRam, segment: u16, offset: u16, prefix: u16) {
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
    if !ram.store(prefix, 0x80, count) {
        return;
    }
    let mut index = 0u16;
    while index <= u16::from(count) && index < 0x7E {
        let Some(byte) = ram.load(tail_segment, tail_offset.wrapping_add(1).wrapping_add(index))
        else {
            return;
        };
        if !ram.store(prefix, 0x81u16.wrapping_add(index), byte) {
            return;
        }
        index = index.wrapping_add(1);
    }
}

fn place_child(
    image: &[u8],
    ram: &mut GuestRam,
    executable: bool,
    segment: u16,
) -> Option<CpuState> {
    if executable {
        super::mz::load(image, ram, segment)
    } else {
        place_com(image, ram, segment)
    }
}

fn allocate(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let asked = cpu.wide(Slot::Base);
    let origin = session.break_at;
    let room = super::memory::CONVENTIONAL_END.saturating_sub(origin);
    if asked == 0 || asked > room {
        cpu.set_wide(Slot::Base, room);
        fail(cpu, session, 8);
        return;
    }
    if asked != 0xFFFF && origin < session.image_limit {
        let span = session.image_limit.saturating_sub(origin);
        ram.hold_image(origin, span);
    }
    session.break_at = origin.wrapping_add(asked);
    session.block = origin;
    cpu.set_wide(Slot::Accumulator, origin);
    cpu.set_carry(false);
}

fn resize_block(cpu: &mut CpuState, session: &mut Session) {
    let block = cpu.selector(Segment::Extra);
    let asked = cpu.wide(Slot::Base);
    let room = super::memory::CONVENTIONAL_END.saturating_sub(block);
    if asked == 0 || asked > room {
        cpu.set_wide(Slot::Base, room);
        fail(cpu, session, 8);
        return;
    }
    let end = block.saturating_add(asked);
    if end > session.kept_end {
        session.kept_end = end;
    }
    cpu.set_carry(false);
}

fn note_image(session: &mut Session, image: &[u8], load_segment: u16) {
    let paragraphs = u16::try_from(image.len().div_ceil(16)).unwrap_or(u16::MAX);
    session.image_limit = load_segment.saturating_add(paragraphs).saturating_add(1);
    session.kept_end = 0;
}

fn resident_end(image: &[u8], load_segment: u16) -> u16 {
    if image.len() >= 28 && image[0] == b'M' && image[1] == b'Z' {
        let header = u16::from_le_bytes([image[8], image[9]]);
        let pages = u16::from_le_bytes([image[4], image[5]]);
        let last = u16::from_le_bytes([image[2], image[3]]);
        let extra = u16::from_le_bytes([image[10], image[11]]);
        let bytes = if pages == 0 {
            0
        } else if last == 0 {
            u32::from(pages) * 512
        } else {
            u32::from(pages - 1) * 512 + u32::from(last)
        };
        let payload = bytes.saturating_sub(u32::from(header) * 16);
        let loaded = u16::try_from(payload.div_ceil(16)).unwrap_or(u16::MAX);
        let stack = u16::from_le_bytes([image[14], image[15]]);
        let image_end = load_segment
            .saturating_add(loaded)
            .saturating_add(extra)
            .saturating_add(1);
        let near_end = load_segment.saturating_add(stack).saturating_add(0x1000);
        return if near_end > image_end { near_end } else { image_end };
    }
    let paragraphs = u16::try_from(image.len().div_ceil(16)).unwrap_or(u16::MAX);
    load_segment.saturating_add(paragraphs).saturating_add(1)
}

pub(super) fn spawn(
    image: &[u8],
    args: &[u8],
    program: &[u8; 11],
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
    if !super::mz::plant_environment(ram, SHELL_CHILD, program) {
        return 0xFF;
    }
    let arena = session.break_at;
    let saved_limit = session.image_limit;
    let saved_kept = session.kept_end;
    note_image(session, image, SHELL_CHILD.saturating_add(0x0010));
    if image.len() > 32 * 1024 {
        let load_segment = SHELL_CHILD.saturating_add(0x0010);
        let image_end = resident_end(image, load_segment);
        if image_end > session.break_at && image_end < super::memory::CONVENTIONAL_END {
            session.break_at = image_end;
        }
    }
    if !args.is_empty() {
        let prefix = SHELL_CHILD;
        let count = args.len().min(126);
        if !ram.store(prefix, 0x80, (count + 1) as u8)
            || !ram.store(prefix, 0x81, b' ')
        {
            session.break_at = arena;
            session.image_limit = saved_limit;
            session.kept_end = saved_kept;
            return 0xFF;
        }
        let mut index = 0usize;
        while index < count {
            if !ram.store(prefix, 0x82u16.wrapping_add(index as u16), args[index]) {
                session.break_at = arena;
                session.image_limit = saved_limit;
                session.kept_end = saved_kept;
                return 0xFF;
            }
            index += 1;
        }
        if !ram.store(prefix, 0x82u16.wrapping_add(index as u16), b'\r') {
            session.break_at = arena;
            session.image_limit = saved_limit;
            session.kept_end = saved_kept;
            return 0xFF;
        }
    }
    let mut inherited = [false; HANDLE_SLOTS];
    let mut slot = 0usize;
    while slot < HANDLE_SLOTS {
        inherited[slot] = session.opens[slot].is_some();
        slot += 1;
    }
    let saved_psp = session.current_psp;
    let saved_stdio = session.stdio;
    session.current_psp = SHELL_CHILD;
    let code = match drive(&mut cpu, ram, session, 8_000_000) {
        End::Stopped(code) => code,
        _ => 0xFF,
    };
    session.current_psp = saved_psp;
    flush_stdio(session);
    session.stdio = saved_stdio;
    slot = 0;
    while slot < HANDLE_SLOTS {
        if !inherited[slot] {
            if let Some(opened) = session.opens[slot] {
                let _ = flush_opened(&opened);
            }
            session.opens[slot] = None;
        }
        slot += 1;
    }
    session.break_at = arena;
    session.image_limit = saved_limit;
    session.kept_end = saved_kept;
    code
}

fn place_com(image: &[u8], ram: &mut GuestRam, segment: u16) -> Option<CpuState> {
    ram.release_image();
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

fn system_values(cpu: &mut CpuState, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        0 => {
            cpu.set_low(Slot::Data, session.break_check);
            cpu.set_carry(false);
        }
        1 => {
            let requested = cpu.low(Slot::Data);
            if requested < 2 {
                session.break_check = requested;
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
        }
        2 => {
            let requested = cpu.low(Slot::Data);
            if requested < 2 {
                session.extended_break = requested;
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
        }
        5 => {
            cpu.set_low(Slot::Data, 1);
            cpu.set_carry(false);
        }
        _ => cpu.set_carry(true),
    }
}

fn parse_filename(cpu: &mut CpuState, ram: &mut GuestRam) {
    let options = cpu.low(Slot::Accumulator);
    let text_segment = cpu.selector(Segment::Data);
    let mut cursor = cpu.wide(Slot::Source);
    if options & 1 != 0 {
        while let Some(byte) = ram.load(text_segment, cursor) {
            if !leading_separator(byte) {
                break;
            }
            cursor = cursor.wrapping_add(1);
        }
    }
    let first = ram.load(text_segment, cursor).unwrap_or(0);
    let second = ram.load(text_segment, cursor.wrapping_add(1)).unwrap_or(0);
    if second == b':' {
        let letter = ascii_upper(first);
        if !letter.is_ascii_uppercase() {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_wide(Slot::Source, cursor);
            cpu.set_carry(false);
            return;
        }
        let _ = ram.store(
            cpu.selector(Segment::Extra),
            cpu.wide(Slot::Dest),
            letter - b'A' + 1,
        );
        cursor = cursor.wrapping_add(2);
    } else if options & 2 == 0 {
        let _ = ram.store(cpu.selector(Segment::Extra), cpu.wide(Slot::Dest), 0);
    }
    let mut wild = false;
    let name = read_name(ram, text_segment, &mut cursor, &mut wild);
    let dotted = ram.load(text_segment, cursor) == Some(b'.');
    if dotted {
        cursor = cursor.wrapping_add(1);
    }
    let extension = read_extension(ram, text_segment, &mut cursor, &mut wild, dotted);
    let record = cpu.selector(Segment::Extra);
    let base = cpu.wide(Slot::Dest);
    if name.written || options & 4 == 0 {
        write_field(ram, record, base.wrapping_add(1), &name.bytes, 8);
    }
    if extension.written || options & 8 == 0 {
        write_field(ram, record, base.wrapping_add(9), &extension.bytes, 3);
    }
    cpu.set_low(Slot::Accumulator, if wild { 1 } else { 0 });
    cpu.set_wide(Slot::Source, cursor);
    cpu.set_carry(false);
}

struct ParsedField {
    bytes: [u8; 8],
    written: bool,
}

fn read_name(
    ram: &GuestRam,
    segment: u16,
    cursor: &mut u16,
    wild: &mut bool,
) -> ParsedField {
    read_field(ram, segment, cursor, wild, 8, true)
}

fn read_extension(
    ram: &GuestRam,
    segment: u16,
    cursor: &mut u16,
    wild: &mut bool,
    dotted: bool,
) -> ParsedField {
    if !dotted {
        return ParsedField {
            bytes: [b' '; 8],
            written: false,
        };
    }
    read_field(ram, segment, cursor, wild, 3, false)
}

fn read_field(
    ram: &GuestRam,
    segment: u16,
    cursor: &mut u16,
    wild: &mut bool,
    width: usize,
    stop_on_dot: bool,
) -> ParsedField {
    let mut bytes = [b' '; 8];
    let mut index = 0usize;
    let mut written = false;
    while index < width {
        let Some(byte) = ram.load(segment, *cursor) else {
            break;
        };
        if byte == b'*' {
            *wild = true;
            written = true;
            while index < width {
                bytes[index] = b'?';
                index += 1;
            }
            *cursor = cursor.wrapping_add(1);
            while let Some(next) = ram.load(segment, *cursor) {
                if (stop_on_dot && next == b'.') || field_end(next) {
                    break;
                }
                *cursor = cursor.wrapping_add(1);
            }
            break;
        }
        if byte == b'?' {
            *wild = true;
            written = true;
            bytes[index] = b'?';
            index += 1;
            *cursor = cursor.wrapping_add(1);
            continue;
        }
        if (stop_on_dot && byte == b'.') || field_end(byte) {
            break;
        }
        written = true;
        bytes[index] = ascii_upper(byte);
        index += 1;
        *cursor = cursor.wrapping_add(1);
    }
    ParsedField { bytes, written }
}

fn write_field(ram: &mut GuestRam, segment: u16, offset: u16, bytes: &[u8; 8], width: usize) {
    let mut index = 0usize;
    while index < width {
        let _ = ram.store(segment, offset.wrapping_add(index as u16), bytes[index]);
        index += 1;
    }
}

fn field_end(byte: u8) -> bool {
    byte <= 0x20
        || matches!(
            byte,
            b'"' | b'+'
                | b','
                | b'/'
                | b':'
                | b';'
                | b'<'
                | b'='
                | b'>'
                | b'['
                | b'\\'
                | b']'
                | b'|'
        )
}

fn leading_separator(byte: u8) -> bool {
    field_end(byte) && byte != b'.' && byte != b':'
}

fn ascii_upper(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte.wrapping_sub(b'a').wrapping_add(b'A')
    } else {
        byte
    }
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
    let mut text = [0u8; 64];
    let mut length = 0usize;
    while let Some(byte) = ram.load(segment, offset) {
        if byte == b'$' {
            break;
        }
        text[length] = byte;
        length += 1;
        offset = offset.wrapping_add(1);
        if length == text.len() {
            crate::bios::mscon::con_write_bytes(&text);
            length = 0;
        }
    }
    if length > 0 {
        crate::bios::mscon::con_write_bytes(&text[..length]);
    }
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

fn load_fcb(ram: &GuestRam, segment: u16, offset: u16) -> Option<[u8; 37]> {
    let mut raw = [0u8; 37];
    for (index, byte) in raw.iter_mut().enumerate() {
        *byte = ram.load(segment, offset.wrapping_add(index as u16))?;
    }
    Some(raw)
}

fn fcb_name(raw: &[u8; 37]) -> [u8; 11] {
    let mut name = [b' '; 11];
    name.copy_from_slice(&raw[1..12]);
    name
}

fn fcb_delete(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    if super::disk::remove_file(&fcb_name(&raw)) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_low(Slot::Accumulator, 0xFF);
    }
}

fn fcb_open(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let (time, date) = super::disk::stamp(&name).unwrap_or((0, 0));
    if raw[0] == 0 && !ram.store(segment, offset, 1) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let size = (len as u32).to_le_bytes();
    let time_bytes = time.to_le_bytes();
    let date_bytes = date.to_le_bytes();
    let fields = [
        (0x0C, 0),
        (0x0D, 0),
        (0x0E, 128),
        (0x0F, 0),
        (0x10, size[0]),
        (0x11, size[1]),
        (0x12, size[2]),
        (0x13, size[3]),
        (0x14, date_bytes[0]),
        (0x15, date_bytes[1]),
        (0x16, time_bytes[0]),
        (0x17, time_bytes[1]),
        (0x20, 0),
    ];
    for (at, byte) in fields {
        if !ram.store(segment, offset.wrapping_add(at), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn fcb_read(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let current = u32::from(raw[0x20]);
    let index = block.saturating_mul(128).saturating_add(current);
    let start = index.saturating_mul(record);
    if start >= len as u32 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let take = record.min((len as u32) - start);
    if start >= 128 || start + take > 128 {
        cpu.set_low(Slot::Accumulator, 2);
        return;
    }
    let mut file = [0u8; 128];
    if super::disk::read_file(&name, &mut file).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let byte = if copied < take {
            file[(start + copied) as usize]
        } else {
            0
        };
        if !ram.store(dta_segment, dta_offset.wrapping_add(copied as u16), byte) {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        }
        copied += 1;
    }
    let next = current + 1;
    let (block_next, record_next) = if next >= 128 {
        (block + 1, 0u8)
    } else {
        (block, next as u8)
    };
    let block_bytes = (block_next as u16).to_le_bytes();
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), record_next)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    cpu.set_low(Slot::Accumulator, if take < record { 3 } else { 0 });
    cpu.set_carry(false);
}

fn fcb_write(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let current = u32::from(raw[0x20]);
    let index = block.saturating_mul(128).saturating_add(current);
    let start = index.saturating_mul(record);
    let end = start.saturating_add(record);
    if record > 128 || len > 128 || end > 128 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && super::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let Some(byte) = ram.load(dta_segment, dta_offset.wrapping_add(copied as u16)) else {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        };
        file[(start + copied) as usize] = byte;
        copied += 1;
    }
    let new_len = (len as u32).max(end) as usize;
    if !super::disk::write_file(&name, &file[..new_len]) {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let size = (new_len as u32).to_le_bytes();
    for (index, byte) in size.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x10 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    let next = current + 1;
    let (block_next, record_next) = if next >= 128 {
        (block + 1, 0u8)
    } else {
        (block, next as u8)
    };
    let block_bytes = (block_next as u16).to_le_bytes();
    if !ram.store(segment, offset.wrapping_add(0x0C), block_bytes[0])
        || !ram.store(segment, offset.wrapping_add(0x0D), block_bytes[1])
        || !ram.store(segment, offset.wrapping_add(0x20), record_next)
    {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn fcb_create(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    if !super::disk::write_named(&fcb_name(&raw), b"", 0x20) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    fcb_open(cpu, ram);
}

fn fcb_rename(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let mut to = [b' '; 11];
    to.copy_from_slice(&raw[0x12..0x1D]);
    if super::disk::rename_file(&fcb_name(&raw), &to) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        cpu.set_low(Slot::Accumulator, 0xFF);
    }
}

fn fcb_random_read(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let record = fcb_record(&raw);
    let start = random_index(&raw, record).saturating_mul(record);
    if start >= len as u32 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let take = record.min((len as u32) - start);
    if start >= 128 || start + take > 128 {
        cpu.set_low(Slot::Accumulator, 2);
        return;
    }
    let mut file = [0u8; 128];
    if super::disk::read_file(&name, &mut file).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let byte = if copied < take {
            file[(start + copied) as usize]
        } else {
            0
        };
        if !ram.store(dta_segment, dta_offset.wrapping_add(copied as u16), byte) {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        }
        copied += 1;
    }
    cpu.set_low(Slot::Accumulator, if take < record { 3 } else { 0 });
    cpu.set_carry(false);
}

fn fcb_random_write(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let name = fcb_name(&raw);
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let record = fcb_record(&raw);
    let start = random_index(&raw, record).saturating_mul(record);
    let end = start.saturating_add(record);
    if record > 128 || len > 128 || end > 128 {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let mut file = [0u8; 128];
    if len > 0 && super::disk::read_file(&name, &mut file[..len]).is_none() {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    }
    let (dta_segment, dta_offset) = dta_target(session);
    let mut copied = 0u32;
    while copied < record {
        let Some(byte) = ram.load(dta_segment, dta_offset.wrapping_add(copied as u16)) else {
            cpu.set_low(Slot::Accumulator, 2);
            return;
        };
        file[(start + copied) as usize] = byte;
        copied += 1;
    }
    let new_len = (len as u32).max(end) as usize;
    if !super::disk::write_file(&name, &file[..new_len]) {
        cpu.set_low(Slot::Accumulator, 1);
        return;
    }
    let size = (new_len as u32).to_le_bytes();
    for (index, byte) in size.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x10 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn fcb_place(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let Some(raw) = load_fcb(ram, segment, offset) else {
        return;
    };
    let block = u32::from(u16::from_le_bytes([raw[0x0C], raw[0x0D]]));
    let index = block.saturating_mul(128).saturating_add(u32::from(raw[0x20]));
    let bytes = index.to_le_bytes();
    for (index, byte) in bytes.into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + index as u16), byte) {
            return;
        }
    }
    cpu.set_carry(false);
}

fn fcb_record(raw: &[u8; 37]) -> u32 {
    let record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 { 128 } else { record }
}

fn random_index(raw: &[u8; 37], record: u32) -> u32 {
    u32::from_le_bytes([
        raw[0x21],
        raw[0x22],
        raw[0x23],
        if record < 64 { raw[0x24] } else { 0 },
    ])
}

fn fcb_size(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let mut raw = [0u8; 37];
    for (index, byte) in raw.iter_mut().enumerate() {
        let Some(loaded) = ram.load(segment, offset.wrapping_add(index as u16)) else {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        };
        *byte = loaded;
    }
    let mut name = [b' '; 11];
    name.copy_from_slice(&raw[1..12]);
    let mut record = u32::from(u16::from_le_bytes([raw[0x0E], raw[0x0F]]));
    if record == 0 {
        record = 128;
    }
    let Some(len) = super::disk::file_len(&name) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        return;
    };
    let records = (len as u32).div_ceil(record);
    for (index, byte) in records.to_le_bytes().into_iter().enumerate() {
        if !ram.store(segment, offset.wrapping_add(0x21 + index as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            return;
        }
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn file_time(cpu: &mut CpuState, session: &mut Session) {
    let sub = cpu.low(Slot::Accumulator);
    if sub > 1 {
        fail(cpu, session, 1);
        return;
    }
    let handle = cpu.wide(Slot::Base);
    if handle < 3 {
        fail(cpu, session, 6);
        return;
    }
    let looked = session.opens.get(usize::from(handle - 3)).and_then(|slot| {
        slot.as_ref()
            .map(|opened| (opened.name, opened.named, opened.console))
    });
    let Some((name, named, console)) = looked else {
        fail(cpu, session, 6);
        return;
    };
    if !named || console {
        fail(cpu, session, 1);
        return;
    }
    if sub == 0 {
        let Some((time, date)) = super::disk::stamp(&name) else {
            fail(cpu, session, 2);
            return;
        };
        cpu.set_wide(Slot::Count, time);
        cpu.set_wide(Slot::Data, date);
    } else {
        let time = cpu.wide(Slot::Count);
        let date = cpu.wide(Slot::Data);
        if !super::disk::set_stamp(&name, time, date) {
            fail(cpu, session, 5);
            return;
        }
    }
    cpu.set_carry(false);
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
    let Some(name) = file_component(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    let action = cpu.low(Slot::Accumulator);
    if action == 0 {
        let Some(attr) = super::disk::attribute(&name) else {
            cpu.set_wide(Slot::Accumulator, 2);
            cpu.set_carry(true);
            return;
        };
        cpu.set_wide(Slot::Count, u16::from(attr));
        cpu.set_carry(false);
        return;
    }
    if action == 1 && super::disk::set_attribute(&name, cpu.low(Slot::Count)) {
        cpu.set_carry(false);
        return;
    }
    cpu.set_wide(Slot::Accumulator, 5);
    cpu.set_carry(true);
}

fn truename(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let text = bytes_at(ram, cpu.selector(Segment::Data), cpu.wide(Slot::Source));
    let built = match canonical_path(&text, session.drive) {
        Ok(built) => built,
        Err(code) => {
            fail(cpu, session, code);
            return;
        }
    };
    let segment = cpu.selector(Segment::Extra);
    let mut offset = cpu.wide(Slot::Dest);
    let mut index = 0usize;
    while index < built.len {
        if !ram.store(segment, offset, built.bytes[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if ram.store(segment, offset, 0) {
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

struct Canon {
    bytes: [u8; 80],
    len: usize,
}

fn canonical_path(text: &[u8; 64], current: u8) -> Result<Canon, u16> {
    let mut cursor = 0usize;
    while cursor < text.len() && text[cursor] == b' ' {
        cursor += 1;
    }
    let mut end = cursor;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    if cursor >= end {
        return Err(3);
    }
    let mut drive = current;
    if cursor + 1 < end && text[cursor + 1] == b':' {
        let letter = ascii_upper(text[cursor]);
        if !letter.is_ascii_uppercase() {
            return Err(15);
        }
        drive = letter - b'A';
        if drive > 1 {
            return Err(15);
        }
        cursor += 2;
    }
    if drive > 1 {
        return Err(15);
    }
    let absolute = cursor < end && (text[cursor] == b'\\' || text[cursor] == b'/');
    let mut parts = [[0u8; 12]; 16];
    let mut lengths = [0u8; 16];
    let mut count = 0usize;
    if !absolute {
        let mut prefix = [0u8; 64];
        let prefix_len = super::disk::current_path(&mut prefix);
        take_parts(&prefix[..prefix_len], &mut parts, &mut lengths, &mut count)?;
    }
    take_parts(&text[cursor..end], &mut parts, &mut lengths, &mut count)?;
    let mut bytes = [0u8; 80];
    bytes[0] = b'A' + drive;
    bytes[1] = b':';
    bytes[2] = b'\\';
    let mut used = 3usize;
    let mut index = 0usize;
    while index < count {
        if index > 0 {
            if used >= bytes.len() {
                return Err(3);
            }
            bytes[used] = b'\\';
            used += 1;
        }
        let length = usize::from(lengths[index]);
        let mut pos = 0usize;
        while pos < length {
            if used >= bytes.len() {
                return Err(3);
            }
            bytes[used] = parts[index][pos];
            used += 1;
            pos += 1;
        }
        index += 1;
    }
    Ok(Canon { bytes, len: used })
}

fn take_parts(
    text: &[u8],
    parts: &mut [[u8; 12]; 16],
    lengths: &mut [u8; 16],
    count: &mut usize,
) -> Result<(), u16> {
    let mut start = 0usize;
    while start <= text.len() {
        let mut stop = start;
        while stop < text.len() && text[stop] != b'\\' && text[stop] != b'/' {
            stop += 1;
        }
        push_part(parts, lengths, count, &text[start..stop])?;
        if stop >= text.len() {
            break;
        }
        start = stop + 1;
    }
    Ok(())
}

fn push_part(
    parts: &mut [[u8; 12]; 16],
    lengths: &mut [u8; 16],
    count: &mut usize,
    raw: &[u8],
) -> Result<(), u16> {
    if raw.is_empty() || raw == b"." {
        return Ok(());
    }
    if raw == b".." {
        if *count > 0 {
            *count -= 1;
        }
        return Ok(());
    }
    if *count >= parts.len() {
        return Err(3);
    }
    let mut name = [0u8; 12];
    let mut used = 0usize;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut dotted = false;
    for &byte in raw {
        let upper = ascii_upper(byte);
        if upper == b'.' {
            if dotted || used == 0 {
                return Err(3);
            }
            dotted = true;
            name[used] = b'.';
            used += 1;
            continue;
        }
        if !path_char(upper) {
            return Err(3);
        }
        if dotted {
            ext += 1;
            if ext > 3 {
                return Err(3);
            }
        } else {
            base += 1;
            if base > 8 {
                return Err(3);
            }
        }
        name[used] = upper;
        used += 1;
    }
    if used == 0 || (dotted && ext == 0) {
        return Err(3);
    }
    parts[*count] = name;
    lengths[*count] = used as u8;
    *count += 1;
    Ok(())
}

fn path_char(byte: u8) -> bool {
    byte.is_ascii_uppercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'-'
                | b'@'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'}'
                | b'~'
        )
}

fn current_directory(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let drive = cpu.low(Slot::Data);
    if drive > 1 {
        fail(cpu, session, 15);
        return;
    }
    let mut path = [0u8; 64];
    let length = super::disk::current_path(&mut path);
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Source);
    let mut index = 0usize;
    while index < length {
        if !ram.store(segment, offset, path[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if ram.store(segment, offset, 0) {
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn find_first(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let text = guest_text(cpu, ram);
    if let Some(letter) = drive_letter(&text)
        && letter != b'A'
    {
        fail(cpu, session, 15);
        return;
    }
    let Some((dir, template)) = search_spec(&text) else {
        fail(cpu, session, 3);
        return;
    };
    let attr = cpu.low(Slot::Count);
    let Some(found) = super::disk::find_match(dir, &template, attr, 0) else {
        fail(cpu, session, 2);
        return;
    };
    if write_match(ram, session, dir, &template, attr, &found) {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn find_next(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let (segment, base) = dta_target(session);
    let mut template = [b' '; 11];
    let mut index = 0usize;
    while index < template.len() {
        let Some(byte) = ram.load(segment, base.wrapping_add(1 + index as u16)) else {
            fail(cpu, session, 18);
            return;
        };
        template[index] = byte;
        index += 1;
    }
    let Some(attr) = ram.load(segment, base.wrapping_add(12)) else {
        fail(cpu, session, 18);
        return;
    };
    let Some(start) = load_word(ram, segment, base.wrapping_add(13)) else {
        fail(cpu, session, 18);
        return;
    };
    let Some(dir) = load_word(ram, segment, base.wrapping_add(15)) else {
        fail(cpu, session, 18);
        return;
    };
    let Some(found) = super::disk::find_match(dir, &template, attr, start) else {
        fail(cpu, session, 18);
        return;
    };
    if write_match(ram, session, dir, &template, attr, &found) {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn drive_letter(text: &[u8; 64]) -> Option<u8> {
    if text.len() >= 2 && text[1] == b':' {
        Some(ascii_upper(text[0]))
    } else {
        None
    }
}

fn search_spec(text: &[u8; 64]) -> Option<(u16, [u8; 11])> {
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    if end == 0 {
        return None;
    }
    let mut cursor = 0usize;
    let mut dir = super::disk::working_directory();
    if end >= 2 && text[1] == b':' {
        cursor = 2;
        dir = 0;
    }
    if cursor < end && (text[cursor] == b'\\' || text[cursor] == b'/') {
        dir = 0;
        cursor += 1;
    }
    let mut last = cursor;
    let mut index = cursor;
    while index < end {
        if text[index] == b'\\' || text[index] == b'/' {
            dir = directory_at(dir, &text[last..index])?;
            last = index + 1;
        }
        index += 1;
    }
    if last >= end {
        return None;
    }
    Some((dir, component_template(&text[last..end])))
}

fn directory_at(parent: u16, text: &[u8]) -> Option<u16> {
    if text == b"." || text.is_empty() {
        return Some(parent);
    }
    if text == b".." {
        return Some(super::disk::parent_directory(parent));
    }
    let mut padded = [0u8; 64];
    let count = text.len().min(padded.len() - 1);
    let mut index = 0usize;
    while index < count {
        padded[index] = text[index];
        index += 1;
    }
    let name = dos_name(&padded)?;
    super::disk::directory_cluster(parent, &name)
}

fn component_template(text: &[u8]) -> [u8; 11] {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut fill = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let upper = ascii_upper(text[index]);
        if upper == b'.' && !extension {
            if fill {
                while base < 8 {
                    name[base] = b'?';
                    base += 1;
                }
            }
            extension = true;
            fill = false;
            index += 1;
            continue;
        }
        if upper == b'*' {
            fill = true;
            index += 1;
            continue;
        }
        if !extension && base < 8 {
            name[base] = if fill { b'?' } else { upper };
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = if fill { b'?' } else { upper };
            ext += 1;
        }
        index += 1;
    }
    if fill {
        if extension {
            while ext < 3 {
                name[8 + ext] = b'?';
                ext += 1;
            }
        } else {
            while base < 8 {
                name[base] = b'?';
                base += 1;
            }
        }
    }
    name
}

fn write_match(
    ram: &mut GuestRam,
    session: &Session,
    dir: u16,
    template: &[u8; 11],
    search_attr: u8,
    found: &super::disk::DirMatch,
) -> bool {
    let (segment, base) = dta_target(session);
    if !ram.store(segment, base, 1) {
        return false;
    }
    let mut index = 0usize;
    while index < template.len() {
        if !ram.store(segment, base.wrapping_add(1 + index as u16), template[index]) {
            return false;
        }
        index += 1;
    }
    if !ram.store(segment, base.wrapping_add(12), search_attr) {
        return false;
    }
    let next = found.index.wrapping_add(1).to_le_bytes();
    if !ram.store(segment, base.wrapping_add(13), next[0])
        || !ram.store(segment, base.wrapping_add(14), next[1])
    {
        return false;
    }
    let place = dir.to_le_bytes();
    if !ram.store(segment, base.wrapping_add(15), place[0])
        || !ram.store(segment, base.wrapping_add(16), place[1])
    {
        return false;
    }
    if !ram.store(segment, base.wrapping_add(0x15), found.attr) {
        return false;
    }
    let stamp_time = found.time.to_le_bytes();
    let stored_date = if found.date == 0 { 0x0021 } else { found.date };
    let stamp_date = stored_date.to_le_bytes();
    if !ram.store(segment, base.wrapping_add(0x16), stamp_time[0])
        || !ram.store(segment, base.wrapping_add(0x17), stamp_time[1])
        || !ram.store(segment, base.wrapping_add(0x18), stamp_date[0])
        || !ram.store(segment, base.wrapping_add(0x19), stamp_date[1])
    {
        return false;
    }
    let size = found.size.to_le_bytes();
    let mut part = 0usize;
    while part < size.len() {
        if !ram.store(
            segment,
            base.wrapping_add(0x1A).wrapping_add(part as u16),
            size[part],
        ) {
            return false;
        }
        part += 1;
    }
    let mut shown = [0u8; 13];
    let mut length = 0usize;
    let mut cursor = 0usize;
    while cursor < 8 && found.name[cursor] != b' ' {
        shown[length] = found.name[cursor];
        length += 1;
        cursor += 1;
    }
    if found.name[8] != b' ' {
        shown[length] = b'.';
        length += 1;
        cursor = 8;
        while cursor < 11 && found.name[cursor] != b' ' {
            shown[length] = found.name[cursor];
            length += 1;
            cursor += 1;
        }
    }
    if length + 1 < shown.len() {
        shown[length + 1] = b'\r';
    }
    if length + 2 < shown.len() {
        shown[length + 2] = b'\n';
    }
    if length + 3 < shown.len() {
        shown[length + 3] = b'$';
    }
    let mut slot = 0usize;
    while slot < shown.len() {
        if !ram.store(segment, base.wrapping_add(0x1E).wrapping_add(slot as u16), shown[slot]) {
            return false;
        }
        slot += 1;
    }
    true
}

fn load_word(ram: &GuestRam, segment: u16, offset: u16) -> Option<u16> {
    let low = ram.load(segment, offset)?;
    let high = ram.load(segment, offset.wrapping_add(1))?;
    Some(u16::from_le_bytes([low, high]))
}

fn dta_target(session: &Session) -> (u16, u16) {
    if session.dta_segment == 0 && session.dta_offset == 0 && session.prefix != 0 {
        (session.prefix, 0x80)
    } else {
        (session.dta_segment, session.dta_offset)
    }
}

fn indos_flag(cpu: &mut CpuState, ram: &mut GuestRam) {
    const SEGMENT: u16 = 0x50;
    const OFFSET: u16 = 4;
    if !ram.store(SEGMENT, OFFSET, 0) || !ram.store(SEGMENT, OFFSET.wrapping_add(1), 0) {
        cpu.set_carry(true);
        return;
    }
    cpu.set_selector(Segment::Extra, SEGMENT);
    cpu.set_wide(Slot::Base, OFFSET);
    cpu.set_carry(false);
}

fn drive_info(cpu: &mut CpuState, ram: &mut GuestRam) {
    let drive = cpu.low(Slot::Data);
    if drive > 1 {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    let Some((per_cluster, per_sector, _free, total)) = super::disk::allocation() else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let media = super::disk::media_id();
    if !ram.store(0x50, 0, media) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    cpu.set_low(Slot::Accumulator, per_cluster as u8);
    cpu.set_wide(Slot::Count, per_sector);
    cpu.set_wide(Slot::Data, total);
    cpu.set_selector(Segment::Data, 0x50);
    cpu.set_wide(Slot::Base, 0);
    cpu.set_carry(false);
}

fn report_space(cpu: &mut CpuState) {
    let drive = cpu.low(Slot::Data);
    if drive > 1 {
        cpu.set_wide(Slot::Accumulator, 0xFFFF);
        cpu.set_carry(true);
        return;
    }
    let Some((per_cluster, per_sector, free_units, total_units)) = super::disk::allocation()
    else {
        cpu.set_wide(Slot::Accumulator, 0xFFFF);
        cpu.set_carry(true);
        return;
    };
    cpu.set_wide(Slot::Accumulator, per_cluster);
    cpu.set_wide(Slot::Base, free_units);
    cpu.set_wide(Slot::Count, per_sector);
    cpu.set_wide(Slot::Data, total_units);
    cpu.set_carry(false);
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

fn device_control(cpu: &mut CpuState, session: &Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        cpu.set_carry(true);
        return;
    }
    cpu.set_wide(Slot::Data, file_device_word(session, cpu.wide(Slot::Base)));
    cpu.set_carry(false);
}

fn file_device_word(session: &Session, handle: u16) -> u16 {
    if handle < 3 {
        if let Some(opened) = session.stdio[usize::from(handle)]
            && !opened.console
        {
            return written_file_word(opened.dirty);
        }
        return 0x80D3;
    }
    let Some(slot) = session.opens.get(usize::from(handle - 3)) else {
        return 0x0040;
    };
    let Some(opened) = slot else {
        return 0x0040;
    };
    if opened.console {
        return 0x80D3;
    }
    written_file_word(opened.dirty)
}

fn written_file_word(written: bool) -> u16 {
    if written { 0 } else { 0x0040 }
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
