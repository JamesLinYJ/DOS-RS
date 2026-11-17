//! The native XMS handle value is the offset of its ten-byte descriptor.
//! Keep this page guest-readable and independent of the monitor's mutable state.
use super::{Frame, GuestRam, Slot, XMS, load_base, put, write32};
pub const HEADER: u16 = super::STACK_TOP as u16;
pub const FIRST: u16 = HEADER + 8;
pub const STRIDE: u16 = 10;
pub const FREE_SLOTS: usize = (4096 - 8) / 10 - 32;
// Keep the same physical bytes behind the far pointer when Windows disables paging.
// A second supervisor-only mapping permits publication with CR0.WP enabled.
const SUPERVISOR: u32 = 0x11b000;
pub fn map(base: u32) {
    let page = base + u32::from(HEADER);
    super::set_pte(page / 4096, page | 5);
    super::set_pte(SUPERVISOR / 4096, page | 3);
    super::flush();
}

fn record(at: u32, flags: u8, locks: u8, address: usize, size: usize) {
    put(at, flags);
    put(at + 1, locks);
    write32(at + 2, (address / 1024) as u32);
    write32(at + 6, (size / 1024) as u32);
}
pub fn sync(ram: &mut GuestRam) {
    if !ram.table_dirty || !ram.table_requested {
        return;
    }
    let base = load_base();
    let table = SUPERVISOR;
    let mut at = table + 8;
    let xms = unsafe { &*(&raw const XMS) };
    for entry in xms.descriptors() {
        match entry {
            Some((locks, start, len)) => record(at, 2, locks, start, len),
            None => record(at, 4, 0, 0, 0),
        }
        at += 10;
    }
    for (address, size) in ram.holes() {
        record(at, 1, 0, address, size);
        at += 10;
    }
    let count = (at - table - 8) / 10;
    write32(table, 1 | 10 << 8 | count << 16);
    write32(table + 4, (base << 12) | u32::from(FIRST));
    ram.table_dirty = false;
}
pub fn query(f: &mut Frame, ram: &mut GuestRam) {
    ram.table_requested = true;
    sync(ram);
    f.set_low(Slot::Accumulator, 0x43);
    f.ves = load_base() >> 4;
    f.set_wide(Slot::Base, HEADER);
}
