//! Kernel-owned firmware topology, worker threads and the PCI device registry.

pub(crate) mod pci;
use crate::arch::platform as arch;
use crate::kernel_cell::KernelCell;
use core::sync::atomic::{
    AtomicUsize,
    Ordering::{Acquire, Relaxed, Release},
};
pub(crate) const MAX_CPUS: usize = 8;
static ONLINE: AtomicUsize = AtomicUsize::new(1);
static WORKERS: AtomicUsize = AtomicUsize::new(1);
static IDS: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static WORK: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static PCI: KernelCell<pci::Bus> = KernelCell::new(pci::Bus::new());
pub(crate) fn initialize(handoff: crate::boot::Handoff) {
    for (slot, &id) in handoff.ids[..handoff.count].iter().enumerate() {
        IDS[slot].store(id.into(), Release);
    }
    if let Some(vector) = handoff.vector
        && arch::enable_apic()
    {
        for slot in 1..handoff.count {
            if !arch::start_ap(handoff.ids[slot], vector)
                || !(0..20_000_000).any(|_| {
                    core::hint::spin_loop();
                    online(slot)
                })
            {
                break;
            }
        }
    }
    WORKERS.store(ONLINE.load(Acquire).count_ones() as usize, Release);
    PCI.borrow().scan();
    crate::puts("CPUs online: ");
    number(ONLINE.load(Acquire).count_ones());
    crate::puts(" / ");
    number(handoff.available as u32);
    crate::puts("   PCI functions: ");
    number(PCI.borrow().devices().len() as u32);
    crate::puts("\n");
}
fn online(slot: usize) -> bool {
    ONLINE.load(Acquire) & (1 << slot) != 0
}
pub(crate) extern "efiapi" fn ap_entry(_context: usize, slot: usize) -> ! {
    crate::arch::load_interrupts();
    let (id, _) = arch::boot_cpu();
    if slot == 0 || slot >= MAX_CPUS || IDS[slot].load(Acquire) != usize::from(id) {
        crate::arch::halt();
    }
    ONLINE.fetch_or(1 << slot, Release);
    loop {
        if crate::guest::run::worker_tick(slot) {
            WORK[slot].fetch_add(1, Relaxed);
        } else {
            core::hint::spin_loop();
        }
    }
}
pub(crate) fn worker(terminal: usize) -> usize {
    let cpus = WORKERS.load(Acquire);
    if cpus < 2 {
        0
    } else {
        1 + terminal % (cpus - 1)
    }
}
pub(crate) fn report() {
    for (slot, id) in IDS.iter().enumerate().filter(|(slot, _)| online(*slot)) {
        crate::puts("CPU ");
        number(slot as u32);
        crate::puts("  APIC ");
        number(id.load(Acquire) as u32);
        crate::puts("  guest slices ");
        number(WORK[slot].load(Relaxed) as u32);
        crate::puts("\n");
    }

    PCI.borrow().report();
}
pub(crate) fn number(mut n: u32) {
    let mut buf = [0; 10];
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for b in &buf[at..] {
        crate::putc(*b);
    }
}
pub(crate) fn hex(n: u32, digits: u32) {
    for i in (0..digits).rev() {
        crate::putc(b"0123456789abcdef"[((n >> (4 * i)) & 15) as usize]);
    }
}
