#![no_std]

#[path = "../../../src/dos/fat_cluster.rs"]
pub mod cluster;

// The architecture runtime supplies the BIOS and segment-register operations.
mod firmware {
    unsafe extern "Rust" {
        pub fn bios_hd(lba: u32, seg: u16, write: u16) -> u16;
        pub fn bios_read(chs: u16, head: u16, segment: u16) -> u16;
        pub fn bios_reset();
        pub fn bios_cpu() -> u16;
        pub fn bios_char(value: u16);
        pub fn bios_text(text: &str);
        pub fn bios_key() -> u16;
        pub fn bios_chain() -> !;
        pub fn bios_stop() -> !;
        pub fn bios_run(volume: &mut super::disk::Volume, cs: u16, ip: u16, ss: u16, sp: u16);
        pub fn mem_byte(segment: u16, offset: u16) -> u16;
        pub fn mem_store(segment: u16, offset: u16, value: u16);
    }
}

pub fn cpu() -> u16 {
    unsafe { firmware::bios_cpu() }
}
pub fn print(s: &str) {
    unsafe { firmware::bios_text(s) }
}
pub fn put(c: u16) {
    unsafe { firmware::bios_char(c) }
}
pub fn key() -> u16 {
    unsafe { firmware::bios_key() }
}
pub fn halt() -> ! {
    unsafe { firmware::bios_stop() }
}
pub fn start_kernel() -> ! {
    unsafe { firmware::bios_chain() }
}
pub fn start_exe(volume: &mut disk::Volume, cs: u16, ip: u16, ss: u16, sp: u16) {
    unsafe { firmware::bios_run(volume, cs, ip, ss, sp) }
}
pub fn start_com(volume: &mut disk::Volume) {
    store(PROGRAM, 0xfffe, 0);
    store(PROGRAM, 0xffff, 0);
    unsafe { firmware::bios_run(volume, PROGRAM, 0x100, PROGRAM, 0xfffe) }
}

pub mod disk;
pub(crate) fn sector(lba: u32, write: bool) -> bool {
    unsafe { firmware::bios_hd(lba, BUFFER, write as u16) == 0 }
}
pub const BUFFER: u16 = 0x3000;
pub const PROGRAM: u16 = 0x4000;
pub const COMMAND: u16 = 0x3800;
pub const BATCH: u16 = 0x3900;
pub const KERNEL: u16 = 0x1000;
// Regions handed to Rust contain no host references. The HAL translates
// these segment:offset pairs without manufacturing Rust flat pointers.
pub fn byte(seg: u16, at: u16) -> u16 {
    unsafe { firmware::mem_byte(seg, at) }
}
pub fn store(seg: u16, at: u16, value: u16) {
    unsafe { firmware::mem_store(seg, at, value) }
}
fn word(at: u16) -> u16 {
    byte(BUFFER, at) | (byte(BUFFER, at + 1) << 8)
}

fn read(lba: u16, segment: u16) -> bool {
    // The native boot format is a two-head, 18-sector/track 1.44 MiB floppy.
    if lba >= 2880 {
        return false;
    }
    let chs = ((lba / 36) << 8) | (lba % 18 + 1);
    let mut tries = 3;
    while tries > 0 {
        if unsafe { firmware::bios_read(chs, (lba / 18) % 2, segment) } == 0 {
            return true;
        }
        unsafe {
            firmware::bios_reset();
        }
        tries -= 1;
    }
    false
}
pub fn reserved() -> u16 {
    if !read(0, BUFFER)
        || word(11) != 512
        || byte(BUFFER, 13) != 1
        || byte(BUFFER, 16) != 2
        || word(17) != 224
        || word(19) != 2880
        || word(22) != 9
        || word(24) != 18
        || word(26) != 2
    {
        return 0;
    }
    let n = word(14);
    if n == 0 || n > 18 { 0 } else { n }
}
fn upper(c: u16) -> u16 {
    if c >= 97 && c <= 122 { c - 32 } else { c }
}
// A packed 8.3 filename in COMMAND at offset 128.
pub fn filename(seg: u16, start: u16, len: u16) -> bool {
    let mut i = 0;
    while i < 11 {
        store(COMMAND, 128 + i, 32);
        i += 1;
    }
    i = 0;
    let mut field = 0;
    let mut dot = false;
    while i < len {
        let c = upper(byte(seg, start.wrapping_add(i)));
        if c == 46 {
            if dot || field == 0 {
                return false;
            }
            dot = true;
            field = 8;
        } else {
            if matches!(c,34|42|43|44|47|58..=63|91..=93|124)
                || c <= 32
                || c >= 127
                || field >= 11
                || (!dot && field == 8)
            {
                return false;
            }
            store(COMMAND, 128 + field, c);
            field += 1;
        }
        i += 1;
    }
    len > 0
}

fn ordinary(at: u16) -> bool {
    let first = byte(BUFFER, at);
    first != 0 && first != 0xe5 && byte(BUFFER, at + 11) & 0x18 == 0
}
pub fn list(res: u16) {
    let mut sector = 0;
    while sector < 14 {
        if !read(res + 18 + sector, BUFFER) {
            print("Read error\r\n");
            return;
        }
        let mut at = 0;
        while at < 512 {
            if byte(BUFFER, at) == 0 {
                return;
            }
            if ordinary(at) {
                let mut i = 0;
                while i < 11 {
                    let c = byte(BUFFER, at + i);
                    if i == 8 && c != 32 {
                        put(46);
                    }
                    if c != 32 {
                        put(c);
                    }
                    i += 1;
                }
                print("\r\n");
            }
            at += 32;
        }
        sector += 1;
    }
}

/// Load a root-directory file from its FAT12 cluster chain. Check both the
/// byte count and last FAT entry before handing execution to the caller.
pub fn load(res: u16, dest: u16, max: u16) -> Option<u16> {
    let mut sector = 0;
    while sector < 14 {
        if !read(res + 18 + sector, BUFFER) {
            return None;
        }
        let mut at = 0;
        while at < 512 {
            if byte(BUFFER, at) == 0 {
                return None;
            }
            if ordinary(at) {
                let mut i = 0;
                while i < 11 && byte(BUFFER, at + i) == byte(COMMAND, 128 + i) {
                    i += 1;
                }
                if i == 11 {
                    let size = word(at + 28);
                    if word(at + 30) != 0 || size > max {
                        return None;
                    }
                    return copy(res, word(at + 26), size, dest);
                }
            }
            at += 32;
        }
        sector += 1;
    }
    None
}
fn copy(res: u16, mut cluster: u16, size: u16, mut dest: u16) -> Option<u16> {
    if size == 0 {
        return Some(0);
    }
    let first = res + 32;
    let limit = 2880 - first + 1;
    let mut remaining = size;
    loop {
        if cluster < 2 || cluster > limit || !read(first + cluster - 2, dest) {
            return None;
        }
        let (offset, high) = cluster::map_cluster(limit, cluster)?;
        let fat = res + (offset / 512) as u16;
        let idx = (offset % 512) as u16;
        if !read(fat, BUFFER) {
            return None;
        }
        let low = byte(BUFFER, idx);
        if idx == 511 && !read(fat + 1, BUFFER) {
            return None;
        }
        let raw = low | byte(BUFFER, (idx + 1) % 512) << 8;
        cluster = if high { raw >> 4 } else { raw & 0xfff };
        if remaining <= 512 {
            return if cluster::is_eof(limit, cluster) {
                Some(size)
            } else {
                None
            };
        }
        remaining -= 512;
        dest += 32;
    }
}

/// Write a source string into the scratch command buffer.
pub fn command(s: &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        store(COMMAND, i as u16, bytes[i] as u16);
        i += 1;
    }
    store(COMMAND, i as u16, 0);
}
