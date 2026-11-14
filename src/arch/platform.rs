//! Privileged platform entry points. All addresses in this module are identity mapped.

#[cfg(target_arch = "x86")]
use core::arch::x86::__cpuid;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__cpuid;

pub(crate) fn boot_cpu() -> (u8, bool) {
    let leaf = __cpuid(1);
    ((leaf.ebx >> 24) as u8, leaf.edx & (1 << 9) != 0)
}

pub(crate) fn port32(port: u16) -> u32 {
    unsafe { x86::io::inl(port) }
}
pub(crate) fn put32(port: u16, value: u32) {
    unsafe { x86::io::outl(port, value) }
}

const LAPIC: usize = 0xfee00000;
fn apic_read(reg: usize) -> u32 {
    unsafe { core::ptr::read_volatile((LAPIC + reg) as *const u32) }
}
fn apic_write(reg: usize, value: u32) {
    unsafe { core::ptr::write_volatile((LAPIC + reg) as *mut u32, value) }
    apic_read(0x20);
}

fn local_apic_base() -> u64 {
    unsafe { x86::msr::rdmsr(0x1b) }
}
pub(crate) fn enable_apic() -> bool {
    let msr = local_apic_base();
    if msr & 0xffff_ffff_ffff_f000 != LAPIC as u64 || msr & 0xc00 != 0x800 {
        return false;
    }
    apic_write(0xf0, 0x1ff);
    true
}
fn ipi(id: u8, command: u32) -> bool {
    let ready = || (0..100_000).any(|_| apic_read(0x300) & 0x1000 == 0);
    if !ready() {
        return false;
    }
    apic_write(0x310, u32::from(id) << 24);
    apic_write(0x300, command);
    ready()
}

// PIT channel 2 in one-shot mode. Leave the speaker output disconnected.
pub(crate) fn pit_wait(count: u16) -> bool {
    use super::{inb, outb};
    let saved = inb(0x61);
    outb(0x61, saved & !3);
    outb(0x43, 0xb0);
    outb(0x42, count as u8);
    outb(0x42, (count >> 8) as u8);
    outb(0x61, (saved & !2) | 1);
    let done = (0..2_000_000).any(|_| inb(0x61) & 0x20 != 0);
    outb(0x61, saved);
    done
}

pub(crate) fn start_ap(id: u8, vector: u8) -> bool {
    let start = 0x4600 | u32::from(vector);
    ipi(id, 0xc500)
        && pit_wait(11932)
        && ipi(id, 0x8500)
        && ipi(id, start)
        && pit_wait(240)
        && ipi(id, start)
}

/// A per-activation key for the legacy LIM driver interface. The kernel's
/// isolation boundary is the session address space, independent of this key.
pub(crate) fn session_nonce() -> u32 {
    use core::sync::atomic::{AtomicU32, Ordering};
    static SEQUENCE: AtomicU32 = AtomicU32::new(1);
    let ticks = unsafe { x86::time::rdtsc() };
    (ticks as u32).rotate_left(11)
        ^ (ticks >> 32) as u32
        ^ SEQUENCE.fetch_add(0x9e3779b9, Ordering::Relaxed)
}
