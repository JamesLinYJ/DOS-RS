//! VCPI 1.0 server. Its protected entry shares only the low identity map with the client.
use super::{FIRST, Frame, GuestRam, RAM, Slot, VM, get, load_base, put, read32, write32};
use core::sync::atomic::{AtomicU16, Ordering};
static IRQ_BASES: AtomicU16 = AtomicU16::new(0x7008);
pub fn pic_vectors() -> (u8, u8) {
    let v = IRQ_BASES.load(Ordering::Relaxed);
    (v as u8, (v >> 8) as u8)
}
pub extern "C" fn resume_pic() {
    super::pic(0x50, 0x58);
}
fn pages(ax: u32, dx: u32, ram: &mut GuestRam) -> (u32, u32) {
    let (status, ret) = match ax as u16 {
        0xde03 => (0, ram.vcpi_free()),
        0xde04 => ram.alloc_frame().map_or((0x88, dx), |p| (0, p)),
        0xde05 if ram.release_frame(dx) => (0, dx),
        0xde05 => (0x8a, dx),
        _ => (0x84, dx),
    };
    ((ax & !0xff00) | (status << 8), ret)
}
pub extern "C" fn pm_pages(ax: u32, dx: u32) -> u64 {
    let (ax, dx) = pages(ax, dx, unsafe { &mut *(&raw mut RAM) });
    u64::from(dx) << 32 | u64::from(ax)
}
fn transition(f: &mut Frame, limit: u32) -> Result<(), u8> {
    let at = f.si;
    if at > limit.saturating_sub(24) {
        return Err(0x8b);
    }
    let pd = read32(at);
    if pd == 0 || pd & 4095 != 0 || pd > limit.saturating_sub(4095) {
        return Err(0x8b);
    }
    let gdt = read32(at + 4);
    let idt = read32(at + 8);
    if gdt > limit.saturating_sub(6) || idt > limit.saturating_sub(6) {
        return Err(0x8b);
    }
    let base = load_base();
    write32(base + 0x380, pd);
    for i in 0..6 {
        put(base + 0x384 + i, get(gdt + i));
        put(base + 0x38a + i, get(idt + i));
    }
    for i in 0..10 {
        put(base + 0x390 + i, get(at + 12 + i));
    }
    let (master, slave) = pic_vectors();
    super::pic(master, slave);
    f.flags = 2;
    f.ip = 0x400;
    f.cs = 8;
    // trapret pops these selectors before the same-privilege IRETD.
    f.ds = 16;
    f.es = 16;
    f.fs = 0;
    f.gs = 0;
    Ok(())
}
fn serve(f: &mut Frame, ram: &mut GuestRam) -> Result<(), u8> {
    match f.ax as u8 {
        0 => f.set_wide(Slot::Base, 0x100),
        1 => {
            let pt = f.ves * 16 + u32::from(f.di as u16);
            let gd = f.vds * 16 + u32::from(f.si as u16);
            // Preserve client-owned entries above the HMA.
            for i in 0..272 {
                write32(pt + 4 * i, read32(FIRST + 4 * i));
            }
            for i in 0..16 {
                put(gd + i, get(load_base() + 0x328 + i))
            }
            for i in 16..24 {
                put(gd + i, 0)
            }
            f.set_wide(Slot::Dest, (f.di as u16).wrapping_add(272 * 4));
            f.bx = 0x480;
        }
        2 => f.dx = (ram.highest_address() as u32) & !4095,
        3..=5 => {
            (f.ax, f.dx) = pages(f.ax, f.dx, ram);
            return if f.high(Slot::Accumulator) == 0 {
                Ok(())
            } else {
                Err(f.high(Slot::Accumulator))
            };
        }
        6 => {
            let n = f.cx as u16;
            if n >= 256 {
                return Err(0x8b);
            }
            f.dx = read32(FIRST + 4 * u32::from(n)) & !4095;
        }
        7 => f.bx = unsafe { x86::controlregs::cr0().bits() as u32 },
        8 | 9 => {
            use x86::debugregs::*;
            let buf = f.ves * 16 + u32::from(f.di as u16);
            unsafe {
                if f.ax as u8 == 8 {
                    for (i, v) in [dr0(), dr1(), dr2(), dr3(), 0, 0, dr6().bits(), dr7().0]
                        .into_iter()
                        .enumerate()
                    {
                        write32(buf + 4 * i as u32, v as u32)
                    }
                } else {
                    dr0_write(read32(buf) as usize);
                    dr1_write(read32(buf + 4) as usize);
                    dr2_write(read32(buf + 8) as usize);
                    dr3_write(read32(buf + 12) as usize);
                    dr6_write(Dr6::from_bits_truncate(read32(buf + 24) as usize));
                    dr7_write(Dr7(read32(buf + 28) as usize));
                }
            }
        }
        10 => {
            let (m, s) = pic_vectors();
            f.set_wide(Slot::Base, u16::from(m));
            f.set_wide(Slot::Count, u16::from(s));
        }
        11 => IRQ_BASES.store(
            u16::from_le_bytes([f.bx as u8, f.cx as u8]),
            Ordering::Relaxed,
        ),
        12 => return transition(f, ram.highest_address() as u32),
        _ => return Err(0x84),
    }
    Ok(())
}
pub fn call(f: &mut Frame, ram: &mut GuestRam) {
    let err = serve(f, ram).err().unwrap_or(0);
    if f.flags & VM != 0 {
        f.set_high(Slot::Accumulator, err)
    }
}
