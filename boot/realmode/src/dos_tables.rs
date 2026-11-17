//! Resident DOS tables exposed to callers of INT 21h/52h.
use crate as pc;
use pc::system::{sd, sw};
pub const SEG: u16 = 0x3600;
// DOSMGR converts the list pointer to a 16-bit low-memory address.
// The boot record's temporary sector at 0000:0600 is no longer in use;
// 0900h lies above that and the BIOS data area, below IO.SYS at 1000h.
pub const LIST_SEG: u16 = 0x0090;
pub const LIST: u16 = 0x40;
pub const SFT: u16 = 0x100;
pub const COUNT: u16 = 32;
pub const CON: u32 = 0x36b0_0000;
pub const fn entry(fd: u16) -> u16 {
    SFT + 6 + fd * 0x3b
}

pub fn init() {
    sd((CON >> 16) as u16, 0, pc::system::dw(LIST_SEG, LIST + 0x22));
    sd(LIST_SEG, LIST + 0x22, CON);
    // Both the conventional MCB chain and the system file table have live backing.
    sw(LIST_SEG, LIST - 2, pc::PROGRAM - 1);
    sd(LIST_SEG, LIST + 4, (u32::from(SEG) << 16) | u32::from(SFT));
    sw(LIST_SEG, LIST + 16, 512);
    pc::store(LIST_SEG, LIST + 0x21, 3);
    sw(LIST_SEG, LIST + 0x26, 0x8004);

    let mut i = 0;
    while i < 8 {
        pc::store(
            LIST_SEG,
            LIST + 0x2c + i,
            u16::from(b"NUL     "[i as usize]),
        );
        i += 1;
    }
    sd(SEG, SFT, u32::MAX);
    sw(SEG, SFT + 4, COUNT);
    let mut fd = 0;
    while fd < 5 {
        let s = entry(fd);
        sw(SEG, s, 1);
        sw(SEG, s + 2, 2);
        sw(SEG, s + 5, if fd == 0 { 0x81 } else { 0x82 });
        if fd < 3 {
            sw(SEG, s + 5, 0x80d3);
            sd(SEG, s + 7, CON);
            pc::copy_bytes((CON >> 16) as u16, 10, SEG, s + 0x20, 8);
            sw(SEG, s + 0x28, 0x2020);
            pc::store(SEG, s + 0x2a, 32);
        }
        fd += 1;
    }
}

pub fn psp(segment: u16, parent: u16, top: u16) {
    let mut i = 0;
    while i < 256 {
        pc::store(segment, i, 0);
        i += 1;
    }
    sw(segment, 0, 0x20cd);
    sw(segment, 2, top);
    pc::copy_bytes(0, 0x22 * 4, segment, 0x0a, 12);
    sw(segment, 0x16, parent);
    i = 0;
    while i < 20 {
        let fd = if segment != parent {
            pc::byte(parent, 0x18 + i)
        } else if i < 5 {
            i
        } else {
            255
        };
        pc::store(segment, 0x18 + i, fd);
        i += 1;
    }
    sw(segment, 0x32, 20);
    sw(segment, 0x34, 0x18);
    sw(segment, 0x36, segment);
    sw(segment, 0x40, 7 | 10 << 8);
    pc::store(segment, 0x50, 0xcd);
    sw(segment, 0x51, 0xcb21);
    if segment != parent {
        sw(segment, 0x2c, pc::system::w(parent, 0x2c));
    }
}

pub const SDA: u16 = 0xa0;
pub const INSTANCE: u16 = 0xd0;
const PATCH: u16 = 0x120;
const START: u16 = 0x108;
pub const SAVED_REGS: u16 = 0x130;
const ACTIVE: u16 = 0x138;
const CDS: u16 = 0x210;
const CDS_LEN: u16 = 88;
pub const C_PATH: u16 = CDS + 2 * CDS_LEN + 3;

fn put_words(at: u16, data: &[u16]) {
    let mut i = 0;
    while i < data.len() {
        sw(LIST_SEG, at + 2 * i as u16, data[i]);
        i += 1;
    }
}
/// Set up a DOS data image that Windows can instance separately for each DOS VM.
pub fn extended_init() {
    // LastDrive A..C. Root CDS records for the native FAT drives are updated
    // with the working path on each change; DPB links are filled at mount.
    let mut drive = 0;
    while drive < 3 {
        let at = CDS + drive * CDS_LEN;
        sw(LIST_SEG, at, (65 + drive) | 0x3a00);
        sw(LIST_SEG, at + 2, 92);
        sw(LIST_SEG, at + 67, 0x4000);
        sw(LIST_SEG, at + 79, 2);
        drive += 1;
    }
    sw(LIST_SEG, LIST + 0x16, CDS);
    sw(LIST_SEG, LIST + 0x18, LIST_SEG);
    put_words(
        PATCH,
        &[
            0x0a07,
            SAVED_REGS,
            SAVED_REGS + 2,
            SDA + 1,
            SDA + 0x1e,
            SDA + 0x1f,
            0,
        ],
    );
    put_words(
        INSTANCE,
        &[
            SDA,
            LIST_SEG,
            32,
            pc::layout::CURRENT_PSP,
            pc::COMMAND,
            2,
            256,
            pc::COMMAND,
            1,
            0,
            0,
            0,
        ],
    );
}
pub fn multiplex(ss: u16, sp: u16) -> u16 {
    // Return zero to continue through the installed INT 2Fh vector chain.
    // Broadcasts 1605h/1606h carry the linked startup records; 1607h/BX=15h
    // is a request to DOSMGR, answered using the live data published above.
    use pc::system::w;
    let ax = w(ss, sp);
    let bx = w(ss, sp + 2);
    let cx = w(ss, sp + 4);
    let dx = w(ss, sp + 6);
    if ax == 0x1605 && dx & 1 == 0 && cx == 0 {
        pc::store(LIST_SEG, ACTIVE, 1);
        // First word is the Windows version passed in DI; ES:BX is the chain.
        put_words(
            START,
            &[
                w(ss, sp + 10),
                bx,
                w(ss, sp + 14),
                0,
                0,
                0,
                0,
                INSTANCE,
                LIST_SEG,
            ],
        );
        sw(ss, sp + 2, START);
        sw(ss, sp + 14, LIST_SEG);
        return 0;
    }
    if ax == 0x1606 {
        pc::store(LIST_SEG, ACTIVE, 0);
        return 0;
    }
    if ax != 0x1607 || bx != 0x15 || pc::byte(LIST_SEG, ACTIVE) == 0 {
        return 0;
    }
    match cx {
        0 => {
            sw(ss, sp + 4, 1);
            sw(ss, sp + 6, LIST_SEG);
            sw(ss, sp + 14, LIST_SEG);
            sw(ss, sp + 2, PATCH);
            return 1;
        }
        1 => sw(ss, sp + 2, dx & 0x1e),
        3 if dx == 1 => sw(ss, sp + 4, CDS_LEN),
        4 => sw(ss, sp + 2, 8),
        2 | 3 => {
            sw(ss, sp + 4, 0);
            return 1;
        }
        5 => {
            sw(ss, sp, 0);
            sw(ss, sp + 6, 0);
            return 1;
        }
        _ => return 0,
    }
    sw(ss, sp, 0xb97c);
    sw(ss, sp + 6, 0xa2ab);
    1
}
