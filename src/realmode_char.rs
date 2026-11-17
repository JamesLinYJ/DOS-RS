//! Install an already resolved resident character device into this DOS PSP.
use dosrs_realmode::{
    self as pc,
    dos_tables::{SEG, entry, set_handle},
    store,
    system::{dw, sd, sw, w},
};

pub(super) fn open(ss: u16, sp: u16, psp: u16) {
    let ax = w(ss, sp);
    let mode = if ax >> 8 == 0x3c { 2 } else { ax & 255 };
    let header = dw(0x34a0, 32);
    let seg = (header >> 16) as u16;
    let off = header as u16;
    let fd = super::free_handle(psp);
    let sys = super::free_slot();
    let err = if mode & 7 > 2 {
        12
    } else if fd == super::CLOSED || sys == super::SLOTS {
        4
    } else if w(seg, off + 4) & 0x0800 != 0 {
        pc::control(28, seg, off, 0)
    } else {
        0
    };
    let flags = w(ss, sp + 22);
    if err != 0 {
        sw(ss, sp, err);
        sw(ss, sp + 22, flags | 1);
        return;
    }
    let s = entry(sys);
    let mut i = 0;
    while i < 0x3b {
        store(SEG, s + i, 0);
        i += 1;
    }
    sw(SEG, s, 1);
    sw(SEG, s + 2, mode);
    store(SEG, s + 4, 0x40);
    sw(SEG, s + 5, (w(seg, off + 4) & 0x7f1f) | 0xc0);
    sd(SEG, s + 7, header);
    pc::copy_bytes(seg, off + 10, SEG, s + 0x20, 8);
    sw(SEG, s + 0x28, 0x2020);
    store(SEG, s + 0x2a, 32);
    set_handle(psp, fd, sys);
    sw(ss, sp, fd);
    sw(ss, sp + 22, flags & !1);
}
