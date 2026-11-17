//! Resident 80386 monitor. Rust pointers are offsets from the driver load segment.
#![no_std]
#![no_main]
mod ems;
mod handle_table;
mod high_memory;
#[path = "../../realmode/src/layout.rs"]
mod layout;
mod memory;
mod registers;
mod upper;
mod vcpi;
mod windows;
#[path = "../../../src/dos_memory/xms.rs"]
mod xms;
use memory::{EMS_FRAME, EMS_PAGE, EXT_START, GuestRam, RAM_BYTES};
use registers::{Segment, Slot};
use upper::{Arena, MemoryError, XMS_OWNER};
type CpuState = Frame;
fn session_nonce() -> u32 {
    unsafe {
        let lo = inb(0x40);
        let hi = inb(0x40);
        u32::from(field(0x304)) << 16 | u32::from(hi) << 8 | u32::from(lo)
    }
}

use core::{mem::size_of, ptr};
use x86::{
    controlregs,
    dtables::{DescriptorTablePointer, lidt},
    io::{inb, outb},
    segmentation::SegmentSelector,
};
const FRAME: u32 = 0xd0000;
// rust16-device reserves this downward-growing stack and one page above it.
const STACK_TOP: u32 = 0xe000;
const VM: u32 = 1 << 17;
// The HMA ends at 10FFEFh. Paging, the TSS and IDT occupy 64 KiB
// immediately above it; none of these pages is returned by any allocator.
const DIRECTORY: u32 = 0x110000;
const FIRST: u32 = 0x111000;

const TSS: u32 = 0x11c000;
const IDT: u32 = 0x11f000;
static mut MANAGER: ems::Ems = ems::Ems::new();
static mut XMS: xms::Xms = xms::Xms::with_handles(handle_table::FIRST, handle_table::STRIDE);
static mut UPPER: Arena = Arena::new();
static mut RAM: GuestRam = GuestRam::new();
unsafe extern "C" {
    static vectors: [u32; 256];
    fn swtch(old: *mut usize, new: usize);
    fn trapret();
    static mut __zero_start: u8;
    static mut __zero_end: u8;
}
#[repr(C)]
#[derive(Default)]
struct Frame {
    di: u32,
    si: u32,
    bp: u32,
    unused: u32,
    bx: u32,
    dx: u32,
    cx: u32,
    ax: u32,
    gs: u32,
    fs: u32,
    es: u32,
    ds: u32,
    vector: u32,
    error: u32,
    ip: u32,
    cs: u32,
    flags: u32,
    sp: u32,
    ss: u32,
    ves: u32,
    vds: u32,
    vfs: u32,
    vgs: u32,
}
fn field(off: usize) -> u16 {
    unsafe { ptr::read_unaligned(off as *const u16) }
}
fn load_base() -> u32 {
    u32::from(field(0x304)) << 4
}
fn current_psp() -> u16 {
    word(u32::from(layout::COMMAND) * 16 + u32::from(layout::CURRENT_PSP))
}
fn address(linear: u32) -> *mut u8 {
    linear.wrapping_sub(load_base()) as *mut u8
}
fn get(addr: u32) -> u8 {
    unsafe { ptr::read_volatile(address(addr)) }
}
fn word(addr: u32) -> u16 {
    u16::from_le_bytes([get(addr), get(addr + 1)])
}
fn put(addr: u32, b: u8) {
    unsafe { ptr::write_volatile(address(addr), b) }
}
impl Frame {
    fn code(&self, offset: u32) -> u8 {
        get((self.cs << 4) + ((self.ip + offset) & 0xffff))
    }
    fn push(&mut self, value: u16) {
        self.sp = (self.sp & !0xffff) | u32::from((self.sp as u16).wrapping_sub(2));
        let sp = self.sp as u16;
        put((self.ss << 4) + u32::from(sp), value as u8);
        put(
            (self.ss << 4) + u32::from(sp.wrapping_add(1)),
            (value >> 8) as u8,
        );
    }
    fn reflect(&mut self, vector: u32) {
        self.push(self.flags as u16);
        self.push(self.cs as u16);
        self.push(self.ip as u16);
        self.flags &= !0x300;
        self.ip = word(vector * 4) as u32;
        self.cs = word(vector * 4 + 2) as u32;
    }
}
fn read32(addr: u32) -> u32 {
    unsafe { ptr::read_unaligned(address(addr).cast::<u32>()) }
}
fn write32(addr: u32, val: u32) {
    unsafe { ptr::write_unaligned(address(addr).cast::<u32>(), val) }
}
fn set_pte(n: u32, entry: u32) {
    write32(FIRST + n * 4, entry);
}
fn flush() {
    unsafe { controlregs::cr3_write(DIRECTORY as u64) }
}
fn install_mapping(ram: &GuestRam) {
    for (i, page) in ram.frame.iter().enumerate() {
        let window = FRAME + i as u32 * 0x4000;
        for sub in 0..4 {
            set_pte(
                window / 4096 + sub,
                (page.unwrap_or(window as usize) as u32 + sub * 4096) | 7,
            );
        }
    }
    for i in 0..16 {
        set_pte(256 + i, ((if ram.a20 { 256 } else { 0 }) + i) * 4096 | 7);
    }
    flush();
}
fn take_interrupt_return(f: &mut Frame) {
    let base = f.ss << 4;
    let sp = f.sp as u16;
    f.ip = word(base + u32::from(sp)) as u32;
    f.cs = word(base + u32::from(sp.wrapping_add(2))) as u32;
    f.flags = VM | 0x3002 | (word(base + u32::from(sp.wrapping_add(4))) as u32 & 0x0fd5);
    f.set_wide(Slot::Stack, sp.wrapping_add(6));
}
fn driver(f: &mut Frame) {
    // This interrupt gate owns the non-reentrant monitor state until IRET.
    let ram = unsafe { &mut *(&raw mut RAM) };
    let manager = unsafe { &mut *(&raw mut MANAGER) };
    let previous = (ram.frame, ram.a20);
    match f.ip {
        0x26 => manager.device_command(ram, f),
        0x22 => {
            take_interrupt_return(f);
            if f.high(Slot::Accumulator) == 0xde {
                vcpi::call(f, ram);
                return;
            }
            manager.call(f, ram);
        }
        0x34 => unsafe {
            // Lock counts and zero-length handles can change without moving any pages.
            if matches!(
                f.high(Slot::Accumulator),
                0x09 | 0x89 | 0x0a | 0x0c | 0x0d | 0x0f | 0x8f
            ) {
                ram.table_dirty = true;
            }
            (*(&raw mut XMS)).call(f, ram, &mut *(&raw mut UPPER));
        },
        0x38 => {
            manager.return_call(f, ram);
        }
        0x3c => match f.ax as u16 {
            0x1605 if f.dx & 1 == 0 => windows::broadcast(f),
            0xd7a0 if f.di as u16 == 0x5253 => {
                if f.cx as u16 == 0xf000 {
                    let owner = if f.dx as u16 == 0 { current_psp() } else { f.dx as u16 };
                    high_memory::release_process(owner);
                }
                f.set_wide(Slot::Dest, 0x5352);
                unsafe { (*(&raw mut UPPER)).dos(f) }
            }
            0xd7b0..=0xd7b4 if f.di as u16 == 0x4850 => high_memory::call(f),
            0x4300 => f.set_low(Slot::Accumulator, 0x80),
            0x4309 => handle_table::query(f, ram),
            0x4310 => {
                f.ves = load_base() >> 4;
                f.set_wide(Slot::Base, 0x12);
            }
            _ => {
                let prev = read32(load_base() + 0x3b8);
                if prev != 0 {
                    f.cs = prev >> 16;
                    f.ip = prev & 65535;
                }
            }
        },
        0x212 => windows::finish_broadcast(f),
        0x7f6 => windows::disable(f),
        _ => f.reflect(0xe6),
    }
    // A 386 invalidates its whole TLB on a CR3 reload. Queries and identical
    // mappings leave live translations alone.
    handle_table::sync(ram);
    if previous != (ram.frame, ram.a20) {
        install_mapping(ram);
    }
}
fn pic(master: u8, slave: u8) {
    unsafe {
        let a = inb(0x21);
        let b = inb(0xa1);
        for (port, value) in [
            (0x20, 0x11),
            (0xa0, 0x11),
            (0x21, master),
            (0xa1, slave),
            (0x21, 4),
            (0xa1, 2),
            (0x21, 1),
            (0xa1, 1),
            (0x21, a),
            (0xa1, b),
        ] {
            outb(port, value);
            outb(0x80, 0);
        }
    }
}
fn stop(code: u8, f: &Frame) -> ! {
    for b in b"EMM386 fault " {
        unsafe {
            outb(0xe9, *b);
        }
    }
    for v in [code as u32, f.cs, f.ip, f.flags, f.error] {
        for shift in (0..8).rev() {
            unsafe {
                outb(
                    0xe9,
                    b"0123456789abcdef"[((v >> (shift * 4)) & 15) as usize],
                );
            }
        }
        unsafe {
            outb(0xe9, 32);
        }
    }
    unsafe {
        outb(0xe9, 10);
    }
    loop {
        core::hint::spin_loop();
    }
}
#[unsafe(no_mangle)]
extern "C" fn dos_ems_trap(f: &mut Frame) {
    if f.flags & VM == 0 {
        stop(f.vector as u8, f);
    }
    match f.vector {
        0x50..=0x57 => f.reflect(f.vector - 0x50 + u32::from(vcpi::pic_vectors().0)),
        0x58..=0x5f => f.reflect(f.vector - 0x58 + u32::from(vcpi::pic_vectors().1)),
        13 if f.code(0) == 0xcd => {
            let vector = f.code(1);
            f.ip = (f.ip + 2) & 0xffff;
            if vector == 0xe6 && f.cs == u32::from(field(0x304)) {
                driver(f);
            } else if vector == 0x68 && read32(0x68 * 4) == 0 { /* absent DOS debugger */
            } else {
                f.reflect(vector as u32);
            }
        }
        13 if f.code(0) == 0xf4 => {
            f.ip = (f.ip + 1) & 0xffff;
        }
        0 | 1 | 3 | 4 | 5 | 6 | 7 | 16 => f.reflect(f.vector),
        _ => stop(f.vector as u8, f),
    }
}
#[unsafe(no_mangle)]
#[unsafe(link_section = ".entry")]
extern "C" fn monitor_start() -> ! {
    unsafe {
        let zero = &raw mut __zero_start;
        ptr::write_bytes(zero, 0, (&raw mut __zero_end as usize) - (zero as usize));
        ptr::write_unaligned(0x348 as *mut usize, dos_ems_trap as *const () as usize);
        let base = load_base();
        windows::init(base);
        let bytes = u32::from(field(0x30a)) * 1024;
        let ram = &mut *(&raw mut RAM);
        ram.init((0x100000 + bytes) as usize);
        ptr::write_bytes(address(DIRECTORY), 0, 0x10000);
        high_memory::init(ram);
        for i in 0..(ram.highest_address() as u32 / 4096 + 1) {
            let linear = i * 4096;
            let resident = linear > base && linear < base + STACK_TOP;
            set_pte(
                i,
                linear
                    | if resident || linear >= DIRECTORY {
                        3
                    } else {
                        7
                    },
            );
        }
        for n in 0..=(ram.highest_address() as u32 >> 22) {
            write32(
                DIRECTORY + 4 * n,
                (FIRST + 4096 * n) | if n == 0 { 7 } else { 3 },
            );
        }
        let t = TSS;
        let limit = (size_of::<[u8; 104 + 8192 + 1]>() - 1) as u64;
        let desc = limit
            | ((u64::from(t) & 0xffffff) << 16)
            | (0x89u64 << 40)
            | ((u64::from(t) & 0xff000000) << 32);
        ptr::write_unaligned(0x338 as *mut u64, desc);
        write32(TSS + 4, STACK_TOP);
        put(TSS + 8, 16);
        put(TSS + 102, 104);
        put(TSS + 104 + 8192, 0xff);
        x86::task::load_tr(SegmentSelector::from_raw(24));
        for (i, off) in vectors.iter().enumerate() {
            let ip = u64::from(*off);
            let gate = (ip & 65535) | (8 << 16) | (0x8eu64 << 40) | ((ip >> 16) << 48);
            ptr::write_unaligned(address(IDT + i as u32 * 8).cast::<u64>(), gate);
        }
        let idtr = DescriptorTablePointer {
            base: IDT as *const u64,
            limit: 2047,
        };
        lidt(&idtr);
        ptr::write_unaligned(0x3a0 as *mut DescriptorTablePointer<u64>, idtr);
        write32(base + 0x39c, DIRECTORY);
        write32(base + 0x3a8, vcpi::pm_pages as *const () as u32);
        flush();
        controlregs::cr0_write(controlregs::cr0() | controlregs::Cr0::CR0_ENABLE_PAGING);
        // Real-mode clients detect the name at INT 67h entry + 10.
        ptr::write_unaligned(address(0x67 * 4).cast::<u32>(), (base << 12) | 0x20);
        write32(base + 0x3b8, read32(0x2f * 4));
        write32(0x2f * 4, (base << 12) | 0x3a);
        handle_table::map(base);
        upper::map(ram, &mut *(&raw mut UPPER));
        ems::Ems::reserve_import(ram);
        // The CPU needs A20 physically enabled to reach our tables. The DOS
        // process starts with zero XMS enable references, so its HMA wraps.
        install_mapping(ram);
        ram.finish_reservations();
        write32(base + 0x3ac, vcpi::resume_pic as *const () as u32);
        vcpi::resume_pic();
        let frame = Frame {
            ip: 0x200,
            cs: base >> 4,
            sp: field(0x308) as u32,
            ss: field(0x306) as u32,
            flags: VM | 0x3002,
            ..Default::default()
        };
        // swtch restores its four callee saves and then returns to xv6's trapret.
        #[repr(C)]
        struct Resume {
            context: [usize; 5],
            frame: Frame,
        }
        let resume = Resume {
            context: [0, 0, 0, 0, trapret as *const () as usize],
            frame,
        };
        let mut old = 0;
        swtch(&mut old, &resume as *const _ as usize);
    }
    loop {
        core::hint::spin_loop()
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop()
    }
}
