//! Marshal the guest's file control block and DTA into the shared DOS service.
use super::*;
use crate::vfs::fcb;

pub(super) fn service(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let ah = cpu.high(Slot::Accumulator);
    let attr = if ram.load(segment, offset) == Some(0xff) {
        let attr = ram.load(segment, offset.wrapping_add(6)).unwrap_or(0);
        offset = offset.wrapping_add(7);
        attr
    } else {
        0
    };
    let mut raw = [0; 37];
    for (i, b) in raw.iter_mut().enumerate() {
        let Some(byte) = ram.load(segment, offset.wrapping_add(i as u16)) else {
            cpu.set_low(Slot::Accumulator, 0xff);
            return;
        };
        *b = byte;
    }
    let old = raw;
    let (seg, at) = dta_target(session);
    let (status, count) = fcb::service(
        &mut session.fcbs,
        session.prefix,
        &mut raw,
        ah,
        attr,
        cpu.wide(Slot::Count),
        0x10000 - usize::from(at),
        |start, buf, input| {
            for (i, b) in buf.iter_mut().enumerate() {
                let pos = at.wrapping_add((start + i) as u16);
                if input {
                    let Some(byte) = ram.load(seg, pos) else {
                        return false;
                    };
                    *b = byte;
                } else if !ram.store(seg, pos, *b) {
                    return false;
                }
            }
            true
        },
    );
    for (i, &byte) in raw.iter().enumerate() {
        if old[i] != byte && !ram.store(segment, offset.wrapping_add(i as u16), byte) {
            cpu.set_low(Slot::Accumulator, 0xff);
            return;
        }
    }
    if matches!(ah, 0x27 | 0x28) {
        cpu.set_wide(Slot::Count, count)
    }
    cpu.set_low(Slot::Accumulator, status);
    cpu.set_carry(false);
}
