//! DOS-RS real-mode shell. Native COM programs run with a separate PSP and stack.
#[path = "realmode_env.rs"]
mod environment;
#[path = "realmode_files.rs"]
mod files;
#[path = "realmode_mz.rs"]
mod mz;
use dosrs_realmode as pc;

fn equal(start: u16, len: u16, word: &str) -> bool {
    if len as usize != word.len() {
        return false;
    }
    let mut i = 0;
    while i < len {
        let mut c = pc::byte(pc::COMMAND, start + i);
        if c >= 97 && c <= 122 {
            c -= 32;
        }
        if c != word.as_bytes()[i as usize] as u16 {
            return false;
        }
        i += 1;
    }
    true
}

use pc::disk::Volume;
fn load_file(vol: &Volume, hard: u16, dest: u16, max: u16) -> Option<u16> {
    if hard > 2 {
        return None;
    }
    if hard == 2 {
        vol.load(dest, max)
    } else {
        pc::store(pc::COMMAND, 258, hard);
        if pc::control(23, hard, 0, 0) == 0 {
            return None;
        }
        let r = 0x35e0;
        let size = pc::system::dw(r, 4);
        if size > u32::from(max) {
            return None;
        }
        pc::system::sd(r, 8, 0);
        pc::system::sw(r, 12, size as u16);
        pc::system::sw(r, 14, dest);
        pc::system::sw(r, 16, 0);
        if pc::control(24, hard, 0, 0) == 0 {
            None
        } else {
            Some(size as u16)
        }
    }
}
fn path(vol: &mut Volume, start: u16, end: u16, default: u16) -> Option<u16> {
    let mut at = start;
    let mut hard = default;
    if end - start >= 2 && pc::byte(pc::COMMAND, start + 1) == 58 {
        if equal(start, 1, "C") {
            hard = 2;
        } else if equal(start, 1, "B") && pc::floppies() >= 2 {
            hard = 1;
        } else if equal(start, 1, "A") {
            hard = 0;
        } else {
            return None;
        }
        at += 2;
    }
    if hard == 2 {
        return if vol.path(pc::COMMAND, at, end - at, false) {
            Some(2)
        } else {
            None
        };
    }
    if hard < 2 {
        pc::store(pc::COMMAND, 258, hard);
    }
    if at < end && pc::byte(pc::COMMAND, at) == 92 {
        at += 1;
    }
    if at >= end || !pc::filename(pc::COMMAND, at, end - at) {
        None
    } else {
        Some(hard)
    }
}
fn spaces(mut at: u16, end: u16) -> u16 {
    while at < end && pc::byte(pc::COMMAND, at) == 32 {
        at += 1;
    }
    at
}

fn execute(vol: &mut Volume, hard: &mut u16, n: u16) {
    let mut split = 0;
    while split < n && pc::byte(pc::COMMAND, split) != 32 {
        split += 1;
    }
    if split == 0 || equal(0, split, "REM") {
        return;
    }
    let cd = pc::cdrom::letter();
    if split == 2 && pc::byte(pc::COMMAND, 1) == 58 && pc::byte(pc::COMMAND, 0) & 0xdf == 65 + cd {
        if pc::cdrom::present() || pc::cdrom::probe() {
            *hard = cd
        } else {
            pc::print("CD-ROM unavailable\r\n")
        }
        return;
    }
    let arg = spaces(split, n);
    let explicit = n >= arg + 2 && pc::byte(pc::COMMAND, arg + 1) == 58;
    let optical = if explicit {
        pc::byte(pc::COMMAND, arg) & 0xdf == 65 + cd
    } else {
        *hard == cd
    };
    if optical {
        let at = if explicit { arg + 2 } else { arg };
        if equal(0, split, "CD") {
            if !pc::cdrom::find(pc::COMMAND, at, n, true) {
                pc::print("Directory unavailable\r\n")
            }
            return;
        }
        if equal(0, split, "DIR") {
            if !pc::cdrom::list() {
                pc::print("CD-ROM read failed\r\n")
            }
            return;
        }
        if equal(0, split, "TYPE") {
            if !pc::cdrom::find(pc::COMMAND, at, n, false) || !pc::cdrom::display() {
                pc::print("CD-ROM read failed\r\n")
            }
            return;
        }
    }
    let original = vol.directory();
    if equal(0, split, "CD") {
        let at = spaces(split, n);
        if *hard == 2 && vol.path(pc::COMMAND, at, n - at, true) {
            environment::path_change(pc::COMMAND, at, n);
            return;
        }
        vol.restore(original);
        pc::print("Directory unavailable\r\n");
        return;
    }
    run(vol, hard, n, split, original);
    vol.restore(original);
}
fn run(vol: &mut Volume, hard: &mut u16, n: u16, split: u16, original: u32) {
    if equal(0, split, "B:") {
        if pc::floppies() >= 2 {
            *hard = 1;
            pc::store(pc::COMMAND, 258, 1);
        } else {
            pc::print("B: unavailable\r\n");
        }
        return;
    }
    if equal(0, split, "A:") {
        *hard = 0;
        return;
    }
    if equal(0, split, "C:") {
        if vol.mounted() || vol.mount() {
            *hard = 2;
        } else {
            pc::print("C: unavailable\r\n");
        }
        return;
    }
    if equal(0, split, "COPY") {
        let src = spaces(split, n);
        let mut sep = src;
        while sep < n && pc::byte(pc::COMMAND, sep) != 32 {
            sep += 1;
        }
        let dst = spaces(sep, n);
        if path(vol, dst, n, *hard) == Some(2) && vol.mounted() {
            let mut i = 0;
            while i < 11 {
                pc::store(pc::COMMAND, 160 + i, pc::byte(pc::COMMAND, 128 + i));
                i += 1;
            }
            let destination = vol.directory();
            vol.restore(original);
            if let Some(from) = path(vol, src, sep, *hard) {
                let copied = if from == 2 {
                    vol.duplicate(destination)
                } else if let Some(size) = load_file(vol, from, mz::FILE, 0xfe00) {
                    i = 0;
                    while i < 11 {
                        pc::store(pc::COMMAND, 128 + i, pc::byte(pc::COMMAND, 160 + i));
                        i += 1;
                    }
                    vol.restore(destination);
                    vol.save(mz::FILE, size)
                } else {
                    false
                };
                if copied {
                    pc::print("File copied\r\n");
                    return;
                }
            }
        }
        pc::print("Copy failed\r\n");
        return;
    }
    if equal(0, split, "ECHO") {
        let mut i = spaces(split, n);
        while i < n {
            pc::put(pc::byte(pc::COMMAND, i));
            i += 1;
        }
        pc::print("\r\n");
        return;
    }
    if equal(0, split, "VER") {
        #[cfg(rust16_cpu = "286")]
        pc::print("DOS / 80286\r\n");
        #[cfg(rust16_cpu = "386")]
        pc::print("DOS / 80386\r\n");
    } else if equal(0, split, "DIR") {
        if *hard == 2 {
            if !vol.list() {
                pc::print("Directory read failed\r\n");
            }
        } else {
            pc::store(pc::COMMAND, 258, *hard);
            let r = pc::reserved();
            if r != 0 {
                pc::list(r)
            } else {
                pc::print("Disk unavailable\r\n");
            }
        }
    } else if equal(0, split, "TYPE") {
        let mut at = split;
        while at < n && pc::byte(pc::COMMAND, at) == 32 {
            at += 1;
        }
        if let Some(from) = path(vol, at, n, *hard) {
            if from == 2 {
                if vol.mounted() && vol.type_file() {
                    return;
                }
            } else if let Some(size) = load_file(vol, from, pc::PROGRAM, 0xfe00) {
                let mut i = 0;
                while i < size {
                    pc::put(pc::byte(pc::PROGRAM, i));
                    i += 1;
                }
                return;
            }
        }
        pc::print("File read failed\r\n");
    } else {
        if let Some(from) = path(vol, 0, split, *hard) {
            if from == 2 && !vol.mounted() {
                pc::print("C: unavailable\r\n");
                return;
            }
            let implicit = pc::byte(pc::COMMAND, 136) == 32;
            if implicit {
                extension(67, 79, 77);
            }
            let com = ext_is(67, 79, 77);
            if com {
                if let Some(size) = load_file(vol, from, pc::PROGRAM + 16, 0xfc00) {
                    if size != 0 {
                        pc::store(pc::COMMAND, 256, *hard);
                        files::begin(*hard == 2);
                        if size >= 2 && pc::system::w(pc::PROGRAM + 16, 0) == 0x5a4d {
                            if !mz::open(vol, from, original, split, n) {
                                pc::print("Invalid DOS executable\r\n");
                            }
                        } else {
                            psp(split, n);
                            vol.restore(original);
                            pc::start_com(vol);
                        }
                        files::finish(vol);
                        return;
                    }
                }
            }
            if implicit {
                extension(69, 88, 69);
            }
            if ext_is(69, 88, 69) {
                pc::store(pc::COMMAND, 256, *hard);
                files::begin(*hard == 2);
                if mz::open(vol, from, original, split, n) {
                    files::finish(vol);
                    return;
                }
            }
        }
        pc::print("Unknown command or unreadable executable\r\n");
    }
}

#[path = "realmode_arena.rs"]
mod arena;
fn psp(split: u16, n: u16) {
    pc::control(25, pc::PROGRAM, pc::PROGRAM, arena::end());
    environment::root(pc::PROGRAM);
    pc::system::sd(
        pc::dos_tables::LIST_SEG,
        pc::dos_tables::SDA + 12,
        (u32::from(pc::PROGRAM) << 16) | 0x80,
    );
    pc::store(pc::PROGRAM, 0x80, n - split);
    let mut i = split;
    while i < n {
        pc::store(pc::PROGRAM, 0x81 + i - split, pc::byte(pc::COMMAND, i));
        i += 1;
    }
    pc::store(pc::PROGRAM, 0x81 + n - split, 13);
}
fn ext_is(a: u16, b: u16, c: u16) -> bool {
    pc::byte(pc::COMMAND, 136) == a
        && pc::byte(pc::COMMAND, 137) == b
        && pc::byte(pc::COMMAND, 138) == c
}
fn extension(a: u16, b: u16, c: u16) {
    pc::store(pc::COMMAND, 136, a);
    pc::store(pc::COMMAND, 137, b);
    pc::store(pc::COMMAND, 138, c);
}

#[unsafe(no_mangle)]
pub fn main16() {
    pc::dos_tables::init();
    unsafe extern "Rust" {
        fn dos16_bind_mux();
    }
    // The linker supplies the real-mode entry gate and preserves the prior vector.
    unsafe {
        dos16_bind_mux();
    }
    arena::init();
    let boot_hd = pc::boot_unit() >= 128;
    let mut vol = Volume::EMPTY;
    pc::cdrom::probe();
    let boot = if boot_hd { 2 } else { pc::boot_unit() };
    let mut hard = boot;
    if vol.mount() {
        pc::print(if vol.is32() {
            "C: FAT32\r\n"
        } else {
            "C: FAT16\r\n"
        });
    }
    pc::command("VER");
    execute(&mut vol, &mut hard, 3);
    pc::command("AUTOEXEC.BAT");
    if pc::filename(pc::COMMAND, 0, 12) {
        if let Some(size) = load_file(&vol, boot, pc::BATCH, 2048) {
            let mut i = 0;
            let mut count = 0;
            while i < size {
                let c = pc::byte(pc::BATCH, i);
                i += 1;
                if c == 10 || c == 13 {
                    execute(&mut vol, &mut hard, count);
                    count = 0;
                } else if count < 126 {
                    pc::store(pc::COMMAND, count, c);
                    count += 1;
                }
            }
            execute(&mut vol, &mut hard, count);
        }
    }
    loop {
        // Reload the transient shell into the file staging area. Keeping a
        // second copy permanently used the space now occupied by the DOS stack.
        pc::command("COMMAND.COM");
        pc::filename(pc::COMMAND, 0, 11);
        let cwd = vol.directory();
        vol.root();
        let saved = load_file(&vol, boot, mz::FILE, 0xfe00);
        vol.restore(cwd);
        let Some(n) = saved else {
            pc::print("Missing COMMAND.COM\r\n");
            return;
        };
        psp(0, 0);
        files::begin(hard == 2);
        pc::store(pc::COMMAND, 256, hard);
        pc::store(pc::COMMAND, 257, 0);
        if !mz::run(&mut vol, n) {
            pc::print("Invalid COMMAND.COM\r\n");
            return;
        }
        files::finish(&mut vol);
        let count = pc::byte(pc::COMMAND, 257);
        execute(&mut vol, &mut hard, count);
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    pc::halt()
}

#[unsafe(no_mangle)]
pub fn dos16_mux(ss: u16, sp: u16) -> u16 {
    pc::control(31, ss, sp, 0)
}
