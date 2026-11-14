//! Resident DOS swappable data area shared with DOS multitaskers.
//! Located after the SFT and before the conventional memory arena.
use super::{CpuState, GuestRam, Segment, Session, Slot, store_bytes};

pub(super) use crate::guest::resident::SEGMENT;
const BASE: u16 = 0x20;
const SIZE: u16 = 0x20;

pub(super) fn publish(ram: &mut GuestRam, s: &Session) {
    // Publish the idle critical-error and InDOS flags, followed by the
    // current error drive (zero when no critical error is pending).
    store_bytes(ram, SEGMENT, BASE, &[0, 0, 0]);
    for (offset, value) in [
        (4, s.extended),
        (12, s.dta_offset),
        (14, s.dta_segment),
        (16, s.current_psp),
        (20, s.last_code),
    ] {
        store_bytes(ram, SEGMENT, BASE + offset, &value.to_le_bytes());
    }
    for (offset, value) in [
        (3, s.extended_locus),
        (6, s.extended_action),
        (7, s.extended_class),
        (22, s.drive),
        (23, s.break_check),
    ] {
        ram.store(SEGMENT, BASE + offset, value);
    }
}
pub(super) fn indos(cpu: &mut CpuState) {
    cpu.set_selector(Segment::Extra, SEGMENT);
    cpu.set_wide(Slot::Base, BASE + 1);
    cpu.set_carry(false);
}
pub(super) fn address(cpu: &mut CpuState, ram: &mut GuestRam, s: &Session) {
    publish(ram, s);
    cpu.set_selector(Segment::Data, SEGMENT);
    cpu.set_wide(Slot::Source, BASE);
    cpu.set_wide(Slot::Count, SIZE);
    cpu.set_wide(Slot::Data, SIZE);
    cpu.set_carry(false);
}

const STARTUP: u16 = 0x40;
const INSTANCES: u16 = STARTUP + 18;
const PATCH: u16 = INSTANCES + 12;
const SAVE_DS: u16 = 0x14;
const SAVE_BX: u16 = 0x16;
const CRIT_PATCH: u16 = 0x18;

fn words(ram: &mut GuestRam, at: u16, items: &[u16]) {
    for (i, word) in items.iter().enumerate() {
        store_bytes(ram, SEGMENT, at + i as u16 * 2, &word.to_le_bytes());
    }
}
pub(super) fn startup(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if cpu.wide(Slot::Data) & 1 != 0 {
        return;
    }
    session.instancing = true;
    words(
        ram,
        STARTUP,
        &[
            cpu.wide(Slot::Dest),
            cpu.wide(Slot::Base),
            cpu.selector(Segment::Extra),
            0,
            0,
            0,
            0,
            INSTANCES,
            SEGMENT,
        ],
    );
    words(ram, INSTANCES, &[BASE, SEGMENT, SIZE, 0, 0, 0]);
    words(
        ram,
        PATCH,
        &[
            crate::dos::getset::VERSION.0,
            SAVE_DS,
            SAVE_BX,
            BASE + 1,
            BASE + 0x1e,
            CRIT_PATCH,
        ],
    );
    cpu.set_selector(Segment::Extra, SEGMENT);
    cpu.set_wide(Slot::Base, STARTUP);
}
pub(super) fn save_call(cpu: &CpuState, ram: &mut GuestRam) {
    words(
        ram,
        SAVE_DS,
        &[cpu.selector(Segment::Data), cpu.wide(Slot::Base)],
    );
}
pub(super) fn dosmgr(cpu: &mut CpuState, session: &Session) {
    match cpu.wide(Slot::Count) {
        0 if session.instancing => {
            cpu.set_wide(Slot::Count, 1);
            cpu.set_wide(Slot::Data, SEGMENT);
            cpu.set_selector(Segment::Extra, SEGMENT);
            cpu.set_wide(Slot::Base, PATCH);
        }
        1 if session.instancing => {
            // File operations have no machine-ID check. Console reads poll, native
            // stacks do not use SYSINIT, and mounted drives have fixed media.
            cpu.set_wide(Slot::Base, cpu.wide(Slot::Data) & 0x1e);
            cpu.set_wide(Slot::Accumulator, 0xb97c);
            cpu.set_wide(Slot::Data, 0xa2ab);
        }
        3 if cpu.wide(Slot::Data) == 1 => {
            cpu.set_wide(Slot::Accumulator, 0xb97c);
            cpu.set_wide(Slot::Data, 0xa2ab);
            cpu.set_wide(Slot::Count, CDS_SIZE);
        }
        2 | 3 => cpu.set_wide(Slot::Count, 0),
        4 if session.instancing => {
            cpu.set_wide(Slot::Base, 8); // the SDA is in our instance table
            cpu.set_wide(Slot::Accumulator, 0xb97c);
            cpu.set_wide(Slot::Data, 0xa2ab);
        }
        _ => {}
    }
}

// Three built-in drives. Each CDS holds the ASCIIZ path and a link to its DPB.
pub(super) const CDS_SIZE: u16 = 0x58;
pub(super) const CDS: u16 = 0x200;
pub(super) const DPB: u16 = 0x320;
pub(super) fn drives(ram: &mut GuestRam) {
    let mut next = u32::MAX;
    for drive in (0..3u8).rev() {
        let at = CDS + u16::from(drive) * CDS_SIZE;
        let block = DPB + u16::from(drive) * 33;
        let mut cds = [0; CDS_SIZE as usize];
        if let Some(dir) = crate::guest::disk::drive_directory(drive) {
            cds[..3].copy_from_slice(&[b'A' + drive, b':', b'\\']);
            crate::guest::disk::in_dir(dir, || crate::guest::disk::current_path(&mut cds[3..66]));
            cds[67..69].copy_from_slice(&0x4000u16.to_le_bytes());
            cds[73..75].copy_from_slice(&(dir as u16).to_le_bytes());
            cds[77..79].copy_from_slice(&u16::MAX.to_le_bytes());
            cds[79..81].copy_from_slice(&2u16.to_le_bytes());
            if let Some(d) = crate::guest::disk::in_dir(dir, crate::guest::disk::drive_block) {
                let mut b = [0; 33];
                b[0] = drive;
                b[1] = drive;
                b[2..4].copy_from_slice(&d.bytes_per_sector.to_le_bytes());
                b[4] = d.sectors_per_cluster - 1;
                b[5] = d.shift;
                b[8] = d.fats;
                b[23] = d.media;
                for (off, w) in [
                    (6, d.reserved),
                    (9, d.root_entries),
                    (11, d.data_sector),
                    (13, d.max_cluster),
                    (15, d.sectors_per_fat),
                    (17, d.reserved + d.sectors_per_fat * u16::from(d.fats)),
                    (31, u16::MAX),
                ] {
                    b[off..off + 2].copy_from_slice(&w.to_le_bytes());
                }
                b[25..29].copy_from_slice(&next.to_le_bytes());
                store_bytes(ram, SEGMENT, block, &b);
                next = u32::from(SEGMENT) << 16 | u32::from(block);
                cds[69..73].copy_from_slice(&next.to_le_bytes());
            }
        }
        store_bytes(ram, SEGMENT, at, &cds);
    }
}
