//! Programs driven during the QEMU boot regression suite.
use super::{End, SHELL_CHILD, SHELL_SEGMENT, Session, drive, load_root, put_hex8};
use crate::guest::{disk, memory};

const COM_SEGMENT: u16 = 0x0100;
const EXE_PREFIX: u16 = 0x0300;
const RUN_SEGMENT: u16 = 0x0500;
const WIDE_SEGMENT: u16 = 0x0D00;

pub fn show() {
    disk::boot();
    show_image(b"HELLO   COM", COM_SEGMENT, 128);
    show_image(b"HELLO   EXE", EXE_PREFIX, 128);
    show_image(b"RUN     COM", RUN_SEGMENT, 128);
    if run_shell() {
        report_shell_files();
    }
    show_image(b"OPCHECK COM", WIDE_SEGMENT, 8192);
    disk::boot();
    show_image(b"DOSCALL COM", WIDE_SEGMENT, 8192);
    show_image(b"BIOSCALLCOM", WIDE_SEGMENT, 8192);
    disk::boot();
    show_image(b"COMPRESSCOM", WIDE_SEGMENT, 20000);
    show_saved();
    crate::bios::mscon::con_write_bytes(b"EXE2BIN ");
    show_image(b"EXE2BIN EXE", EXE_PREFIX, 20000);
    show_exec();
    show_image(b"DBOF    EXE", EXE_PREFIX, 60000);
    show_image(b"EXEFIX  EXE", EXE_PREFIX, 60000);
    show_image(b"CONVERT EXE", EXE_PREFIX, 60000);
    show_tailed(b"ATTRIB  EXE", b"HELLO.COM", 60000);
    show_tailed(b"EXEMOD  EXE", b"HELLO.EXE /h", 60000);
    architecture_probe();
}

fn show_tailed(name: &[u8; 11], args: &[u8], limit: u16) {
    run_fixture(name, EXE_PREFIX, args, limit);
}

fn show_exec() {
    run_fixture(b"EXEC    EXE", EXE_PREFIX, b"HELLO.COM", 60000);
}

fn show_saved() {
    let mut body = [0u8; 32];
    let Some(length) = disk::read_file(b"SELECT  DAT", &mut body) else {
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

pub fn run_shell() -> bool {
    disk::boot();
    let mut image = [0u8; 512];
    let Some(length) = disk::read_file(b"COMMAND COM", &mut image) else {
        crate::bios::mscon::con_write_bytes(b"Cannot find COMMAND.COM.\r\n");
        return false;
    };
    let ram = &mut *memory::storage();
    ram.clear();
    let mut session = Session::new();
    let Some(mut cpu) = load_root(
        &image[..length],
        b"COMMAND COM",
        SHELL_SEGMENT,
        &[],
        ram,
        &mut session,
    ) else {
        crate::bios::mscon::con_write_bytes(b"Cannot load COMMAND.COM.\r\n");
        return false;
    };
    super::drivers::configure(ram, &mut session);
    if cfg!(feature = "batch-console") {
        crate::puts("Running AUTOEXEC.BAT\n\n");
    }
    match drive(&mut cpu, ram, &mut session, 100_000) {
        End::Stopped(0) => return true,
        End::Stopped(code) => {
            crate::puts("Shell exit code: ");
            crate::bios::mscon::write_number(code);
            crate::puts("\n");
        }
        End::Fault(opcode) => {
            crate::bios::mscon::con_write_bytes(b"Shell stopped at opcode 0x");
            put_hex8(opcode);
            crate::bios::mscon::con_write_bytes(b"\r\n");
        }
        End::Other => {
            crate::bios::mscon::con_write_bytes(b"Shell stopped on an unsupported interrupt.\r\n")
        }
        End::Loop => {
            crate::bios::mscon::con_write_bytes(b"Shell reached the instruction limit.\r\n")
        }
    }
    false
}

fn report_shell_files() {
    if let Some(size) = disk::file_len(b"SAY     BIN") {
        let mut body = [0u8; 32];
        let length = disk::read_head(b"SAY     BIN", &mut body).unwrap_or(0);
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
    let Some(length) = disk::read_head(b"LIB     LST", &mut body) else {
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
    let Some(size) = disk::file_len(b"PACKED  EXE") else {
        return;
    };
    let mut body = [0u8; 16384];
    let Some(length) = disk::read_head(b"PACKED  EXE", &mut body) else {
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
    let Some(length) = disk::read_head(name, &mut body) else {
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

fn show_image(name: &[u8; 11], segment: u16, limit: u16) {
    run_fixture(name, segment, &[], limit);
}

fn run_fixture(name: &[u8; 11], segment: u16, args: &[u8], limit: u16) {
    let mut image = [0u8; 16384];
    let Some(length) = disk::read_file(name, &mut image) else {
        crate::bios::mscon::con_write_bytes(b"GUEST MISSING\r\n");
        return;
    };
    let ram = &mut *memory::storage();
    ram.clear();
    let mut session = Session::new();
    let Some(mut cpu) = load_root(&image[..length], name, segment, args, ram, &mut session) else {
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

fn architecture_probe() {
    disk::boot();
    let ram = &mut *memory::storage();
    ram.clear();
    let mut session = Session::new();
    let image = include_bytes!("../../../tests/architecture.com");
    let Some(mut cpu) = load_root(image, b"ARCHTESTCOM", SHELL_CHILD, &[], ram, &mut session)
    else {
        crate::puts("GUEST ARCH LOAD FAILED\n");
        return;
    };
    let result = drive(&mut cpu, ram, &mut session, 10_000);
    match result {
        End::Stopped(0) => crate::puts("GUEST ARCH OK\n"),
        End::Stopped(code) => {
            crate::puts("GUEST ARCH FAIL ");
            put_hex8(code);
            crate::puts("\n");
        }
        End::Fault(op) => {
            crate::puts("GUEST ARCH FAULT ");
            put_hex8(op);
            crate::puts(" IP=");
            crate::put_hex16(cpu.instruction());
            crate::puts("\n");
        }
        End::Other => crate::puts("GUEST ARCH INT\n"),
        End::Loop => crate::puts("GUEST ARCH LOOP\n"),
    }
}
