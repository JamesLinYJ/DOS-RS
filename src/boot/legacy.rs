//! SeaBIOS/PC BIOS entry with a flat low 4 GiB map and VGA text mode.
use super::{Handoff, secondary};
use crate::platform::MAX_CPUS;
use core::ptr::NonNull;

fn physical(addr: usize, len: usize) -> Option<&'static [u8]> {
    // BIOS memory and ACPI reclaimable pages stay reserved for kernel lifetime.
    (addr >= 0x400 && len <= 1024 * 1024 && addr.checked_add(len)? <= u32::MAX as usize)
        .then(|| unsafe { core::slice::from_raw_parts(addr as *const u8, len) })
}
fn dword(d: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(d[n..n + 4].try_into().unwrap())
}
fn checksum(d: &[u8]) -> bool {
    d.iter().fold(0u8, |s, b| s.wrapping_add(*b)) == 0
}
fn table(addr: usize) -> Option<&'static [u8]> {
    let head = physical(addr, 36)?;
    let full = physical(addr, dword(head, 4) as usize)?;
    (full.len() >= 36 && checksum(full)).then_some(full)
}
fn madt() -> Option<&'static [u8]> {
    let ebda = u16::from_le_bytes(physical(0x40e, 2)?.try_into().ok()?) as usize * 16;
    for p in (ebda..ebda + 1024)
        .step_by(16)
        .chain((0xe0000..0x100000).step_by(16))
    {
        let Some(r) = physical(p, 20) else { continue };
        if &r[..8] != b"RSD PTR " || !checksum(r) {
            continue;
        }
        let root = table(dword(r, 16) as usize)?;
        if &root[..4] != b"RSDT" {
            continue;
        }
        for item in root[36..].chunks_exact(4) {
            if let Some(t) = table(dword(item, 0) as usize)
                && t.len() >= 44
                && &t[..4] == b"APIC"
            {
                return Some(t);
            }
        }
    }
    None
}
#[unsafe(no_mangle)]
pub extern "efiapi" fn dos_rs_bios_entry() -> ! {
    let bsp = crate::arch::platform::boot_cpu().0;
    let mut h = Handoff {
        ids: [0; MAX_CPUS],
        count: 1,
        available: 1,
        vector: None,
    };
    h.ids[0] = bsp;
    if let Some(table) = madt() {
        let mut entries = &table[44..];
        while entries.len() >= 2 {
            let len = usize::from(entries[1]);
            if len < 2 || len > entries.len() {
                break;
            }
            if entries[0] == 0 && len >= 8 && dword(entries, 4) & 1 != 0 && entries[3] != bsp {
                h.available += 1;
                if h.count < MAX_CPUS {
                    h.ids[h.count] = entries[3];
                    h.count += 1;
                }
            }
            entries = &entries[len..];
        }
    }
    // The BIOS loader vacates this page below the relocated kernel and above the IVT.
    if h.count > 1 {
        h.vector = secondary::prepare(NonNull::new(0x8000 as *mut u8).unwrap());
    }
    crate::bios::screen::vga_text();
    crate::arch::outb(0x21, 0xff);
    crate::arch::outb(0xa1, 0xff);
    crate::kmain(h, "Legacy BIOS")
}
