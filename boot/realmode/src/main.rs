#![no_std]
#![no_main]
mod console;
mod font;
mod startup;
use dosrs_realmode::{
    self as bios,
    system::{Media, dw, w},
};
unsafe extern "Rust" {
    fn bind_resident_io();
}
#[unsafe(no_mangle)]
pub fn io_dispatch(op: u16, a: u16, b: u16, _c: u16) -> u16 {
    match op {
        16 => bios::optical::probe() as u16,
        17 => bios::optical::present() as u16,
        18 => bios::optical::list() as u16,
        19 | 20 => bios::optical::find(a, b, _c, op == 20) as u16,
        21 => bios::optical::display() as u16,
        23 => {
            let Some(m) = Media::device(a) else { return 0 };
            let mut name = [0u8; 11];
            let mut i = 0;
            while i < 11 {
                name[i] = bios::byte(bios::COMMAND, 128 + i as u16) as u8;
                i += 1;
            }
            let Some(f) = m.lookup(&name) else { return 0 };
            bios::system::sd(0x35e0, 0, f.first);
            bios::system::sd(0x35e0, 4, f.size);
            1
        }
        24 => {
            let s = 0x35e0;
            let Some(m) = Media::device(a) else { return 0 };
            let f = bios::system::File {
                first: dw(s, 0),
                size: dw(s, 4),
                contiguous: false,
            };
            m.fetch(&f, dw(s, 8), w(s, 12), w(s, 14), w(s, 16)) as u16
        }
        22 => {
            let s = 0x35e0;
            let n =
                bios::optical::fetch(dw(s, 0), dw(s, 4), dw(s, 8), w(s, 12), w(s, 14), w(s, 16));
            bios::system::sd(s, 20, n);
            (n != u32::MAX) as u16
        }
        _ => console::control(op, a, b),
    }
}
fn boot() -> Option<()> {
    let unit = bios::boot_unit();
    bios::store(bios::COMMAND, 258, if unit < 128 { unit } else { 0 });
    bios::store(bios::COMMAND, 260, if unit >= 128 { unit } else { 128 });
    bios::cache::init();
    let cpu = bios::cpu();
    if cpu < 286 {
        return None;
    }
    let media = Media::open(bios::byte(0x6000, 0) >= 128)?;
    let font = media.lookup(b"GLYPHS  FNT")?;
    let pages = media.map_sectors(&font, console::MAP, 2560);
    console::init(unit, pages);
    startup::apply(&media);
    unsafe {
        bind_resident_io();
    }
    bios::store(bios::COMMAND, 265, 0xa5);
    let file = media.lookup(b"MSDOS   SYS")?;
    let seg = 0x3280;
    if !media.fetch(&file, 0, 16, seg, 0) || dw(seg, 0) != 0x4b534f44 {
        return None;
    }
    let entry = if cpu == 286 { 4 } else { 10 };
    let n = w(seg, entry + 4);
    if n == 0 || n > 0xfff0 || !media.fetch(&file, dw(seg, entry), n, bios::KERNEL, 0) {
        return None;
    }
    bios::start_kernel()
}
#[unsafe(no_mangle)]
pub fn main16() {
    let _ = boot();
    bios::print("Cannot load MSDOS.SYS (80286 or newer required).\r\n");
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    bios::halt()
}
