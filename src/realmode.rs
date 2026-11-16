//! DOS-RS real-mode shell. Native COM programs run with a separate PSP and stack.
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
fn load_file(res: u16, vol: &Volume, hard: bool, dest: u16, max: u16) -> Option<u16> {
    if hard {
        vol.load(dest, max)
    } else {
        pc::load(res, dest, max)
    }
}
fn path(vol: &mut Volume, start: u16, end: u16, default: bool) -> Option<bool> {
    let mut at = start;
    let mut hard = default;
    if end - start >= 2 && pc::byte(pc::COMMAND, start + 1) == 58 {
        if equal(start, 1, "C") {
            hard = true;
        } else if equal(start, 1, "A") {
            hard = false;
        } else {
            return None;
        }
        at += 2;
    }
    if hard {
        return if vol.path(pc::COMMAND, at, end - at, false) {
            Some(true)
        } else {
            None
        };
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

fn execute(res: u16, vol: &mut Volume, hard: &mut bool, n: u16) {
    let mut split = 0;
    while split < n && pc::byte(pc::COMMAND, split) != 32 {
        split += 1;
    }
    if split == 0 {
        return;
    }
    let original = vol.directory();
    if equal(0, split, "CD") {
        let at = spaces(split, n);
        if *hard && vol.path(pc::COMMAND, at, n - at, true) {
            return;
        }
        vol.restore(original);
        pc::print("Directory unavailable\r\n");
        return;
    }
    run(res, vol, hard, n, split, original);
    vol.restore(original);
}
fn run(res: u16, vol: &mut Volume, hard: &mut bool, n: u16, split: u16, original: u32) {
    if equal(0, split, "A:") {
        *hard = false;
        return;
    }
    if equal(0, split, "C:") {
        if vol.mounted() {
            *hard = true;
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
        if path(vol, dst, n, *hard) == Some(true) && vol.mounted() {
            let mut i = 0;
            while i < 11 {
                pc::store(pc::COMMAND, 160 + i, pc::byte(pc::COMMAND, 128 + i));
                i += 1;
            }
            let destination = vol.directory();
            vol.restore(original);
            if let Some(from) = path(vol, src, sep, *hard) {
                let copied = if from {
                    vol.duplicate(destination)
                } else if let Some(size) = pc::load(res, mz::FILE, 0xfe00) {
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
    if equal(0, split, "VER") {
        #[cfg(rust16_cpu = "286")]
        pc::print("DOS / 80286\r\n");
        #[cfg(rust16_cpu = "386")]
        pc::print("DOS / 80386\r\n");
    } else if equal(0, split, "DIR") {
        if *hard {
            if !vol.list() {
                pc::print("Directory read failed\r\n");
            }
        } else {
            pc::list(res);
        }
    } else if equal(0, split, "TYPE") {
        let mut at = split;
        while at < n && pc::byte(pc::COMMAND, at) == 32 {
            at += 1;
        }
        if let Some(from) = path(vol, at, n, *hard) {
            if from {
                if vol.mounted() && vol.type_file() {
                    return;
                }
            } else if let Some(size) = pc::load(res, pc::PROGRAM, 0xfe00) {
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
            if from && !vol.mounted() {
                pc::print("C: unavailable\r\n");
                return;
            }
            let implicit = pc::byte(pc::COMMAND, 136) == 32;
            if implicit {
                extension(67, 79, 77);
            }
            let com = ext_is(67, 79, 77);
            if com {
                if let Some(size) = load_file(res, vol, from, pc::PROGRAM + 16, 0xfc00) {
                    if size != 0 {
                        psp(split, n);
                        vol.restore(original);
                        files::begin(*hard);
                        pc::start_com(vol);
                        files::finish(vol);
                        return;
                    }
                }
            }
            if implicit {
                extension(69, 88, 69);
            }
            if ext_is(69, 88, 69) {
                if let Some(size) = load_file(res, vol, from, mz::FILE, 0xfe00) {
                    psp(split, n);
                    vol.restore(original);
                    files::begin(*hard);
                    if mz::run(vol, size) {
                        files::finish(vol);
                        return;
                    }
                    pc::print("Invalid MZ executable\r\n");
                    return;
                }
            }
        }
        pc::print("Unknown command or unreadable executable\r\n");
    }
}

fn psp(split: u16, n: u16) {
    let mut i = 0;
    while i < 256 {
        pc::store(pc::PROGRAM, i, 0);
        i += 1;
    }
    pc::store(pc::PROGRAM, 0, 0xcd);
    pc::store(pc::PROGRAM, 1, 0x20);
    pc::store(pc::PROGRAM, 3, 0x60);
    pc::store(pc::PROGRAM, 0x40, crate::version::WORD);
    pc::store(pc::PROGRAM, 0x41, crate::version::WORD >> 8);
    pc::store(pc::PROGRAM, 0x80, n - split);
    i = split;
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
    let res = pc::reserved();
    if res == 0 {
        pc::print("Invalid boot volume\r\n");
        return;
    }
    let mut vol = Volume::EMPTY;
    let mut hard = false;
    if vol.mount() {
        pc::print(if vol.is32() {
            "C: FAT32\r\n"
        } else {
            "C: FAT16\r\n"
        });
    }
    pc::command("VER");
    execute(res, &mut vol, &mut hard, 3);
    pc::command("AUTOEXEC.BAT");
    if pc::filename(pc::COMMAND, 0, 12) {
        if let Some(size) = pc::load(res, pc::BATCH, 2048) {
            let mut i = 0;
            let mut count = 0;
            while i < size {
                let c = pc::byte(pc::BATCH, i);
                i += 1;
                if c == 10 || c == 13 {
                    execute(res, &mut vol, &mut hard, count);
                    count = 0;
                } else if count < 126 {
                    pc::store(pc::COMMAND, count, c);
                    count += 1;
                }
            }
            execute(res, &mut vol, &mut hard, count);
        }
    }
    loop {
        pc::print(if hard { "C:\\> " } else { "A:\\> " });
        let mut len = 0;
        loop {
            let c = pc::key();
            if c == 13 {
                pc::print("\r\n");
                break;
            }
            if c == 8 && len > 0 {
                len -= 1;
                pc::print("\x08 \x08");
            }
            if c >= 32 && c <= 126 && len < 126 {
                pc::store(pc::COMMAND, len, c);
                len += 1;
                pc::put(c);
            }
        }
        execute(res, &mut vol, &mut hard, len);
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    pc::halt()
}
