//! Private DOS multiplex service for allocating and copying owned high RAM.
//! The usual monitor and VCPI ABI retain their original two-level page tables.
use super::{Frame, GuestRam, Slot, address, field, get, load_base, read32, word, write32};
use core::ptr;
use x86::controlregs::{self as cr, Cr0, Cr4};

// This frame is outside the HMA, EMS/XMS pool and legacy page tables.
// Bytes 0..31 are the four hardware PDPTEs. The remaining bytes are supervisor
// state, including a private copy of the firmware map from real-mode INIT.
const ROOT: u32 = 0x11a000;
const WIDTH: u32 = ROOT + 32;
const MAP_BYTES: u32 = ROOT + 36;
const SLOTS: u32 = ROOT + 64;
const MAP: u32 = ROOT + 512;
const HANDLES: u32 = 16;
const PAGE: u64 = 4096;
const HIGH: u64 = 1 << 32;

fn quad(at: u32) -> u64 {
    u64::from(read32(at)) | u64::from(read32(at + 4)) << 32
}
fn store_quad(at: u32, value: u64) {
    write32(at, value as u32);
    write32(at + 4, (value >> 32) as u32);
}
fn records() -> impl Iterator<Item = (u64, u64, bool)> {
    (MAP..MAP + read32(MAP_BYTES)).step_by(24).filter_map(|at| {
        let base = quad(at);
        let size = quad(at + 8);
        let usable = read32(at + 16) == 1;
        if size == 0 || read32(at + 20) & 1 == 0 {
            return None;
        }
        let end = if usable {
            base.checked_add(size)?
        } else {
            base.saturating_add(size)
        };
        Some((base, end, usable))
    })
}
fn slots() -> impl Iterator<Item = u32> {
    (0..HANDLES).map(|i| SLOTS + i * 12)
}
fn page_at_or_above(floor: u64) -> Option<u64> {
    let ceiling = 1u64 << read32(WIDTH);
    // Accept complete pages inside usable RAM only. Overlapping firmware
    // reservations win irrespective of the original E820 record order.
    records()
        .filter(|r| r.2)
        .filter_map(|(start, end, _)| {
            let mut candidate = floor.max(start).max(HIGH).checked_add(PAGE - 1)? & !(PAGE - 1);
            let limit = end.min(ceiling);
            while candidate.checked_add(PAGE)? <= limit {
                if let Some(skip) = records()
                    .filter(|&(lo, hi, usable)| !usable && lo < candidate + PAGE && hi > candidate)
                    .map(|r| r.1)
                    .max()
                {
                    candidate = skip.checked_add(PAGE - 1)? & !(PAGE - 1);
                } else if slots().any(|slot| quad(slot) == candidate) {
                    candidate += PAGE;
                } else {
                    return Some(candidate);
                }
            }
            None
        })
        .min()
}
pub(super) fn init(ram: &mut GuestRam) {
    let width = get(load_base() + 0x3c0).min(48);
    let size = u32::from(field(0x3c2));
    if width <= 32 || size == 0 || size > 64 * 24 || size % 24 != 0 {
        return;
    }
    // The entire destination frame was cleared by monitor_start.
    unsafe {
        ptr::copy_nonoverlapping(
            address(load_base() + super::STACK_TOP),
            address(MAP),
            size as usize,
        )
    };
    write32(MAP_BYTES, size);
    write32(WIDTH, u32::from(width));
    if page_at_or_above(HIGH).is_none() {
        return;
    }
    // This is the first allocator call, before EMS/XMS/UMB reservations.
    if let Some(pd) = ram.allocate_ext(PAGE as usize) {
        debug_assert!(pd.is_multiple_of(4096));
        unsafe { ptr::write_bytes(address(pd as u32), 0, 4096) };
        write32(ROOT, pd as u32 | 1);
        write32(pd as u32, 0x83); // 2 MiB identity map, supervisor only
    }
}

enum Transfer {
    Clear,
    ToGuest,
    FromGuest,
}
fn transfer(physical: u64, guest: u32, len: usize, op: Transfer) {
    let pd = read32(ROOT) & !4095;
    let huge_mask = (1 << 21) - 1;
    store_quad(pd + 8, (physical & !huge_mask) | 0x83);
    let high = address((1 << 21) + (physical & huge_mask) as u32);
    let low = address(guest);
    // Entered from an interrupt gate with IF clear. Code, stack and descriptor
    // tables are below 2 MiB; guest buffers are confined to identity-mapped
    // process conventional memory. Never return to V86 with the PAE root live.
    unsafe {
        let old0 = cr::cr0();
        let old3 = cr::cr3();
        let old4 = cr::cr4();
        let without_paging = old0 & !Cr0::CR0_ENABLE_PAGING;
        cr::cr0_write(without_paging);
        cr::cr4_write(old4 | Cr4::CR4_ENABLE_PAE | Cr4::CR4_ENABLE_PSE);
        cr::cr3_write(u64::from(ROOT));
        cr::cr0_write(old0);
        match op {
            Transfer::Clear => ptr::write_bytes(high, 0, len),
            Transfer::ToGuest => ptr::copy_nonoverlapping(high, low, len),
            Transfer::FromGuest => ptr::copy_nonoverlapping(low, high, len),
        }
        cr::cr0_write(without_paging);
        cr::cr4_write(old4);
        cr::cr3_write(old3);
        cr::cr0_write(old0);
    }
}

fn process_buffer(addr: u32, len: u32, owner: u16) -> bool {
    let end = u32::from(word(u32::from(super::layout::FONT_META) * 16
        + u32::from(super::layout::FONT_WORKSPACE_SEGMENT))).saturating_sub(1);
    let mut header = u32::from(super::layout::PROGRAM - 1);
    while header < end {
        let at = header << 4;
        if !matches!(get(at), b'M' | b'Z') {
            break;
        }
        let next = header + 1 + u32::from(word(at + 3));
        if next > end {
            break;
        }
        if word(at + 1) == owner && addr >= at + 16 && addr + len <= next << 4 {
            return true;
        }
        header = next;
    }
    false
}

pub(super) fn release_process(owner: u16) {
    for slot in slots().filter(|&s| word(s + 10) == owner) {
        store_quad(slot, 0);
    }
}
pub(super) fn call(f: &mut Frame) {
    let function = f.wide(Slot::Accumulator);
    let lo = f.wide(Slot::Base);
    let mid = f.wide(Slot::Count);
    let hi = f.wide(Slot::Data);
    f.set_wide(Slot::Dest, 0x5048);
    f.set_wide(Slot::Accumulator, 0);
    let owner = super::current_psp();
    match function {
        0xd7b0 => {
            f.set_wide(Slot::Base, read32(WIDTH) as u16);
            f.set_wide(Slot::Accumulator, u16::from(read32(ROOT) != 0));
        }
        0xd7b1 if read32(ROOT) != 0 && owner != 0 => {
            let floor = u64::from(lo) | u64::from(mid) << 16 | u64::from(hi) << 32;
            let Some(slot) = slots().find(|&s| quad(s) == 0) else {
                return;
            };
            let Some(page) = page_at_or_above(floor) else {
                return;
            };
            transfer(page, 0, PAGE as usize, Transfer::Clear);
            let generation = (u32::from(word(slot + 8)) >> 4) % 4095 + 1;
            let handle = (generation << 4) | (slot - SLOTS) / 12;
            store_quad(slot, page);
            write32(slot + 8, (u32::from(owner) << 16) | handle);
            f.set_wide(Slot::Accumulator, handle as u16);
            f.set_wide(Slot::Base, page as u16);
            f.set_wide(Slot::Count, (page >> 16) as u16);
            f.set_wide(Slot::Data, (page >> 32) as u16);
        }
        0xd7b2..=0xd7b4 => {
            let slot = SLOTS + u32::from(hi & 15) * 12;
            let page = quad(slot);
            if page == 0 || read32(slot + 8) != (u32::from(owner) << 16 | u32::from(hi)) {
                return;
            }
            if function == 0xd7b4 {
                store_quad(slot, 0);
            } else {
                let offset = u32::from(f.wide(Slot::Source));
                let guest = ((f.ves & 0xffff) << 4) + offset;
                let length = u32::from(mid);
                if u32::from(lo) + length > PAGE as u32
                    || offset + length > 65536
                    || !process_buffer(guest, length, owner)
                {
                    return;
                }
                let direction = if function == 0xd7b2 {
                    Transfer::ToGuest
                } else {
                    Transfer::FromGuest
                };
                transfer(page + u64::from(lo), guest, length as usize, direction);
            }
            f.set_wide(Slot::Accumulator, 1);
        }
        _ => (),
    }
}
