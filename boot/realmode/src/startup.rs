//! Read the boot volume's CONFIG.SYS before the kernel starts AUTOEXEC.BAT.
use crate::console;
use dosrs_realmode::{self as pc, byte, system::Media};
const BUFFER: u16 = 0x3400;
fn matches(at: u16, end: u16, s: &[u8]) -> bool {
    if end - at != s.len() as u16 {
        return false;
    }
    let mut i = 0;
    while i < s.len() {
        let c = byte(BUFFER, at + i as u16);
        let expected = s[i] as u16;
        if (if c >= 97 && c <= 122 { c - 32 } else { c })
            != (if expected >= 97 && expected <= 122 {
                expected - 32
            } else {
                expected
            })
        {
            return false;
        }
        i += 1;
    }
    true
}
fn setting(start: u16, end: u16) -> u16 {
    let names: [&[u8]; 11] = [
        b"DOS=UMB",
        b"DOS=HIGH,UMB",
        b"DOS=LOW,UMB",
        b"SMARTDRV=OFF",
        b"SMARTDRV=ON",
        b"CDROM=OFF",
        b"CDROM=ON",
        b"DISPLAY=CGA",
        b"DISPLAY=EGA",
        b"DISPLAY=VGA",
        b"DISPLAY=MONO",
    ];
    let mut key = 0;
    while key < names.len() {
        if matches(start, end, names[key]) {
            return key as u16;
        }
        key += 1;
    }
    u16::MAX
}
pub fn apply(media: &Media) {
    pc::store(pc::COMMAND, 270, 1);
    pc::store(pc::COMMAND, 272, 0);
    pc::language::set(pc::language::ENGLISH);
    let Some(f) = media.lookup(b"CONFIG  SYS") else {
        return;
    };
    if f.size > 2048 || !media.fetch(&f, 0, f.size as u16, BUFFER, 0) {
        return;
    }
    let mut drivers = crate::devices::Devices::new();
    let mut p = 0;
    while p < f.size as u16 {
        let start = p;
        while p < f.size as u16 && byte(BUFFER, p) != 13 && byte(BUFFER, p) != 10 {
            p += 1
        }
        if p - start >= 7 && matches(start, start + 7, b"DEVICE=") {
            if !drivers.load(media, start + 7, p) {
                pc::print("Device not loaded: ");
                let mut i = start + 7;
                while i < p {
                    pc::put(byte(BUFFER, i));
                    i += 1;
                }
                pc::print("\r\n");
            }
        } else if p - start > 5 && matches(start, start + 5, b"LANG=") {
            let mut id = 0;
            while id < 8 {
                if matches(start + 5, p, pc::language::tag(id).as_bytes()) {
                    pc::language::set(id);
                    break;
                }
                id += 1;
            }
        } else {
            match setting(start, p) {
                0..=2 => pc::store(pc::COMMAND, 272, 1),
                3 => pc::cache::enable(false),
                4 => pc::cache::init(),
                5 => pc::store(pc::COMMAND, 270, 0),
                6 => pc::store(pc::COMMAND, 270, 1),
                7 => console::mode_set(6),
                8 => console::mode_set(16),
                9 => console::mode_set(18),
                10 => console::mode_set(7),
                _ => (),
            }
        }
        if p < f.size as u16 {
            p += 1
        }
    }
}
