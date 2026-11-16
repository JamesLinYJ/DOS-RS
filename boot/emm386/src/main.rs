//! Resident 80386 monitor. Rust pointers are offsets from the driver load segment.
#![no_std]
#![no_main]
mod ems;
use core::{mem::size_of, ptr};
use x86::{
    controlregs,
    dtables::{DescriptorTablePointer, lidt},
    io::{inb, outb},
    segmentation::SegmentSelector,
};
const FRAME: u32 = 0xd0000;
const BACKING: u32 = 0x110000; // keep the HMA available
const VM: u32 = 1 << 17;
#[repr(C, align(4096))]
struct Table([u32; 1024]);
static mut DIRECTORY: Table = Table([0; 1024]);
static mut FIRST: Table = Table([0; 1024]);
static mut TSS: [u8; 104 + 8192 + 1] = [0; 104 + 8192 + 1];
static mut IDT: [u64; 256] = [0; 256];
static mut MANAGER: ems::Manager = ems::Manager::new();
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
fn address(linear: u32) -> *mut u8 {
    linear.wrapping_sub(load_base()) as *mut u8
}
fn phys<T>(p: *const T) -> u32 {
    load_base().wrapping_add(p as u32)
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
fn flush() {
    unsafe { controlregs::cr3_write(phys(&raw const DIRECTORY) as u64) }
}
fn install_mapping(m: &ems::Manager) {
    for (i, p) in m.map.iter().enumerate() {
        let virtual_base = FRAME + i as u32 * 0x4000;
        let backing = if *p == ems::EMPTY {
            virtual_base
        } else {
            BACKING + m.physical(*p).unwrap() as u32 * 0x4000
        };
        for j in 0..4 {
            unsafe {
                FIRST.0[(virtual_base / 4096 + j) as usize] = (backing + j * 4096) | 7;
            }
        }
    }
    flush();
}
fn lower(r: &mut u32, v: u16) {
    *r = (*r & !0xffff) | v as u32;
}
fn service(f: &mut Frame) -> Result<(), u8> {
    // On this single-CPU DOS target the interrupt gate clears IF throughout a request.
    let m = unsafe { &mut *(&raw mut MANAGER) };
    let id = f.dx as u16;
    match (f.ax >> 8) as u8 {
        0x40 => {}
        0x41 => lower(&mut f.bx, (FRAME >> 4) as u16),
        0x42 => {
            lower(&mut f.bx, m.free());
            lower(&mut f.dx, m.total());
        }
        0x43 => {
            let id = m.alloc(f.bx as u16)?;
            lower(&mut f.dx, id);
        }
        0x44 => {
            m.assign(f.ax as u8 as u16, id, f.bx as u16)?;
            install_mapping(m);
        }
        0x45 => {
            m.release(id)?;
            install_mapping(m);
        }
        0x46 => lower(&mut f.ax, 0x32),
        0x47 => m.save(id)?,
        0x48 => {
            m.restore(id)?;
            install_mapping(m);
        }
        0x4b => lower(&mut f.bx, m.allocations().count() as u16),
        0x4c => lower(&mut f.bx, m.count(id)?),
        0x4d => {
            let mut n = 0;
            for (h, pages) in m.allocations() {
                for (j, b) in h
                    .to_le_bytes()
                    .into_iter()
                    .chain(pages.to_le_bytes())
                    .enumerate()
                {
                    put(
                        (f.ves << 4) + u32::from((f.di as u16).wrapping_add(n * 4 + j as u16)),
                        b,
                    );
                }
                n += 1;
            }
            lower(&mut f.bx, n);
        }
        _ => return Err(0x84),
    }
    Ok(())
}
fn pic() {
    unsafe {
        let a = inb(0x21);
        let b = inb(0xa1);
        for (port, value) in [
            (0x20, 0x11),
            (0xa0, 0x11),
            (0x21, 0x50),
            (0xa1, 0x58),
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
        0x50..=0x57 => f.reflect(f.vector - 0x50 + 8),
        0x58..=0x5f => f.reflect(f.vector - 0x58 + 0x70),
        13 if f.code(0) == 0xcd => {
            let vector = f.code(1);
            f.ip = (f.ip + 2) & 0xffff;
            if vector == 0xe6 && f.cs == field(0x304) as u32 && f.ip == 0x22 {
                let error = service(f).err().unwrap_or(0);
                f.ax = (f.ax & !0xff00) | u32::from(error) << 8;
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
        let bytes = u32::from(field(0x30a)) * 1024;
        (*(&raw mut MANAGER)).init(((bytes - 0x10000) / 0x4000).min(ems::MAX as u32) as u16);
        for i in 0..1024 {
            let linear = i as u32 * 4096;
            // The first resident page contains DOS-callable entries and a writable device header.
            // Protect the remaining monitor code, page tables, TSS and ring-zero stack.
            let resident = linear > base && linear < base + 0xd000;
            FIRST.0[i] = linear | if resident || linear >= BACKING { 3 } else { 7 };
        }
        DIRECTORY.0[0] = phys(&raw const FIRST) | 7;
        let t = phys(&raw const TSS);
        let limit = (size_of::<[u8; 104 + 8192 + 1]>() - 1) as u64;
        let desc = limit
            | ((u64::from(t) & 0xffffff) << 16)
            | (0x89u64 << 40)
            | ((u64::from(t) & 0xff000000) << 32);
        ptr::write_unaligned(0x338 as *mut u64, desc);
        ptr::write_unaligned((&raw mut TSS).cast::<u8>().add(4).cast::<u32>(), 0xd000);
        TSS[8] = 16;
        TSS[102] = 104;
        TSS[104 + 8192] = 0xff;
        x86::task::load_tr(SegmentSelector::from_raw(24));
        for i in 0..256 {
            let ip = vectors[i] as u64;
            IDT[i] = (ip & 65535) | (8 << 16) | (0x8eu64 << 40) | ((ip >> 16) << 48);
        }
        lidt(&DescriptorTablePointer {
            base: phys(&raw const IDT) as *const u64,
            limit: 2047,
        });
        flush();
        controlregs::cr0_write(controlregs::cr0() | controlregs::Cr0::CR0_ENABLE_PAGING);
        // Real-mode clients detect the name at INT 67h entry + 10.
        ptr::write_unaligned(address(0x67 * 4).cast::<u32>(), (base << 12) | 0x20);
        pic();
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
