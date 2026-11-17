//! Windows owns protected mode between successful AX=0 and AX=1 callbacks.
//! Both the callback and its far return frame must survive paging being off.
use super::{FIRST, Frame, Slot, field, get, load_base, pic, put, read32, vcpi, write32};

const CALLBACK: u16 = 0x700;
const AFTER_PRIOR: u16 = 0x210;
const SUSPENDED: u32 = 0x2cc;
const V86: u32 = 0x2d8;

pub(super) fn init(base: u32) {
    let segment = base >> 4;
    write32(base + 0x2a0, check_resume_stack as *const () as u32);
    write32(base + 0x2a8, (segment << 16) | 0x7a8);
    // Temporary 16-bit CS/DS/SS descriptors, base=driver, byte limit=FFFF.
    write32(base + 0x2ac, 23);
    write32(base + 0x2ae, base + 0x2b4);
    for (entry, access) in [(0x2bc, 0x9a), (0x2c4, 0x92)] {
        write32(base + entry, (base << 16) | 0xffff);
        write32(base + entry + 4, ((base >> 16) & 0xff) | (access << 8));
    }
    // AX=1 may arrive with a different real-mode stack (e.g. Windows exit).
    // The entry stub supplies that SS:ESP; the shared tail restores all registers.
    for (i, value) in [
        0x7cc, segment, 0x23002, 0, 0, segment, segment, segment, segment,
    ]
    .into_iter()
    .enumerate()
    {
        write32(base + V86 + i as u32 * 4, value);
    }
}

pub(super) fn broadcast(f: &mut Frame) {
    let previous = read32(load_base() + 0x3b8);
    if previous != 0 {
        // PUSHF/CALL FAR previous. Its IRET returns here with CX, ES:BX
        // and DS:SI intact; the original Windows INT frame stays underneath.
        f.push(f.flags as u16);
        f.push(field(0x304));
        f.push(AFTER_PRIOR);
        f.cs = previous >> 16;
        f.ip = previous & 0xffff;
    } else {
        finish_broadcast(f);
    }
}
pub(super) fn finish_broadcast(f: &mut Frame) {
    if f.cx as u16 != 0 {
        return;
    }
    if f.vds as u16 != 0 || f.si as u16 != 0 {
        f.set_wide(Slot::Count, 1); // Windows permits only one V86 owner.
    } else {
        f.vds = u32::from(field(0x304));
        f.set_wide(Slot::Source, CALLBACK);
    }
}
fn visible_without_paging(linear: u32, rights: u32) -> bool {
    let page = linear & !4095;
    read32(FIRST + (linear >> 12) * 4) & (0xffff_f000 | rights) == page | rights
}
fn shared_return_frame(ss: u32, sp: u16) -> bool {
    let stack = |i: u16| (ss << 4) + u32::from(sp.wrapping_add(i));
    // Four segment words + PUSHAD + PUSHFD + the original far return pointer.
    if !(0..48).all(|i| visible_without_paging(stack(i), 7)) {
        return false;
    }
    let cs = u16::from_le_bytes([get(stack(46)), get(stack(47))]);
    let ip = u16::from_le_bytes([get(stack(44)), get(stack(45))]);
    visible_without_paging((u32::from(cs) << 4) + u32::from(ip), 5)
}
extern "C" fn check_resume_stack() -> u32 {
    let base = load_base();
    let ss = read32(base + V86 + 16);
    let sp = read32(base + V86 + 12) as u16;
    if shared_return_frame(ss, sp) {
        return 1;
    }
    // Paging is still off. Report the failure on the real caller's saved FLAGS.
    let addr = (ss << 4) + u32::from(sp.wrapping_add(40));
    put(addr, get(addr) | 1);
    0
}
pub(super) fn disable(f: &mut Frame) {
    let base = load_base();
    if get(base + SUSPENDED) != 0 || !shared_return_frame(f.ss, f.sp as u16) {
        return;
    }
    write32(base + V86 + 12, f.sp);
    write32(base + V86 + 16, f.ss);
    put(base + SUSPENDED, 1);
    let (master, slave) = vcpi::pic_vectors();
    pic(master, slave);
    f.flags = 2;
    f.cs = 8;
    f.ip = 0x438;
    f.ds = 16;
    f.es = 16;
    f.fs = 0;
    f.gs = 0;
}
