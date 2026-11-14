//! Set up the EDK II CPU library's documented MP_CPU_EXCHANGE_INFO ABI.
use crate::platform::MAX_CPUS;
use core::{mem::size_of, ptr::NonNull};
const STACK: usize = if cfg!(feature = "pc386") {
    512 * 1024
} else {
    64 * 1024
};
#[repr(C, align(16))]
struct Stacks([[u8; STACK]; MAX_CPUS]);
static mut STACKS: Stacks = Stacks([[0; STACK]; MAX_CPUS]);
static GDT: [u64; 4] = [
    0,
    0x00cf9a000000ffff,
    0x00cf92000000ffff,
    0x00209a0000000000,
];
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Descriptor {
    limit: u16,
    base: usize,
}
#[repr(C, packed)]
struct Exchange {
    stacks: usize,
    size: usize,
    entry: usize,
    executing: usize,
    index: usize,
    gdt: Descriptor,
    idt: Descriptor,
    base: usize,
    mode: usize,
    code: usize,
    data: usize,
    nx: usize,
    cr3: usize,
    init: usize,
    info: usize,
    context: usize,
    fpu: usize,
    flat: u32,
    cs32: u16,
    long: u32,
    cs64: u16,
    la57: u8,
    sev: u8,
    snp: u8,
    ghcb: usize,
    topo: u8,
    known: u8,
}
#[repr(C)]
#[derive(Default)]
struct LibraryMap {
    start: usize,
    entry: usize,
    len: usize,
    other: [usize; 4],
    flat: usize,
    tail: [usize; 3],
}
unsafe extern "efiapi" {
    fn AsmGetAddressMap(map: &mut LibraryMap);
    fn InitializeFloatingPointUnits();
}
pub(super) fn prepare(page: NonNull<u8>) -> Option<u8> {
    let base = page.as_ptr() as usize;
    if !(4096..0x100000).contains(&base) || base % 4096 != 0 {
        return None;
    }
    let mut lib = LibraryMap::default();
    // Upstream returns pointers into the already relocated loaded image. Only
    // its 16-bit prefix is copied; code above 1 MiB stays at its linked address.
    unsafe {
        AsmGetAddressMap(&mut lib);
    }
    let flat = u32::try_from(lib.start.checked_add(lib.flat)?).ok()?;
    let long = u32::try_from(lib.start.checked_add(lib.entry)?).ok()?;
    let data = lib.flat.next_multiple_of(size_of::<usize>());
    if lib.flat == 0 || lib.flat > lib.len || data.checked_add(size_of::<Exchange>())? > 4096 {
        return None;
    }
    let gdt = &raw const GDT as usize;
    u32::try_from(gdt).ok()?;
    // The trampoline loads CR3 before entering long mode and can address 32 bits.
    let cr3 = unsafe { x86::controlregs::cr3() };
    let cr4 = unsafe { x86::controlregs::cr4() }.bits();
    let nx = if cfg!(target_arch = "x86_64") {
        (unsafe { x86::msr::rdmsr(0xc0000080) } >> 11 & 1) as usize
    } else {
        0
    };
    let exchange = Exchange {
        stacks: &raw mut STACKS as usize,
        size: STACK,
        entry: crate::platform::ap_entry as *const () as usize,
        executing: 0,
        index: 0,
        gdt: Descriptor {
            limit: 31,
            base: gdt,
        },
        idt: Descriptor { limit: 0, base: 0 },
        base,
        mode: lib.entry,
        code: if cfg!(target_arch = "x86") { 8 } else { 24 },
        data: 16,
        nx,
        cr3: u32::try_from(cr3).ok()? as usize,
        init: 1,
        info: 0,
        context: 0,
        fpu: InitializeFloatingPointUnits as *const () as usize,
        flat,
        cs32: 8,
        long,
        cs64: 24,
        la57: ((cr4 >> 12) & 1) as u8,
        sev: 0,
        snp: 0,
        ghcb: 0,
        topo: 0,
        known: 0,
    };
    // The firmware allocated this exclusive executable page below 1 MiB.
    unsafe {
        core::ptr::copy_nonoverlapping(lib.start as *const u8, page.as_ptr(), lib.flat);
        page.as_ptr()
            .add(data)
            .cast::<Exchange>()
            .write_unaligned(exchange);
    }
    Some((base >> 12) as u8)
}
