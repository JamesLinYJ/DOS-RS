//! Map the same PE image used by UEFI and enter its BIOS-specific Rust export.
#![no_std]
#![no_main]
use core::{mem::size_of, ptr};
use x86::dtables::{DescriptorTablePointer, lgdt};
const BASE: usize = 0x1000000;
const STACK: usize = 0x300000;
#[repr(C)]
pub struct BiosInfo {
    len: u32,
    capacity: u32,
    staging: u32,
}
static GDT: [u64; 4] = [
    0,
    0x00cf9a000000ffff,
    0x00cf92000000ffff,
    0x00209a0000000000,
];
#[repr(C, align(4096))]
struct PageTables([[u64; 512]; 6]);
static mut TABLES: PageTables = PageTables([[0; 512]; 6]);
unsafe extern "C" {
    fn InitializeFloatingPointUnits();
    fn InternalSwitchStack(entry: usize, a: usize, b: usize, stack: usize) -> !;
    fn InternalX86EnablePaging64(cs: u32, entry: u64, a: u64, b: u64, stack: u64) -> !;
}

fn word(data: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap())
}
fn dword(data: &[u8], pos: usize) -> usize {
    u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize
}
fn map(file: &[u8], image: &mut [u8]) -> (usize, bool) {
    assert_eq!(&file[..2], b"MZ");
    let pe = dword(file, 0x3c);
    assert_eq!(&file[pe..pe + 4], b"PE\0\0");
    let opt = pe + 24;
    let wide = match (word(file, pe + 4), word(file, opt)) {
        (0x14c, 0x10b) => false,
        (0x8664, 0x20b) => true,
        _ => panic!(),
    };
    assert_eq!(dword(file, opt + if wide { 24 } else { 28 }), BASE);
    if wide {
        assert_eq!(dword(file, opt + 28), 0);
    }
    assert_eq!(dword(file, opt + 56), image.len());
    image.fill(0);
    for i in 0..usize::from(word(file, pe + 6)) {
        let s = opt + usize::from(word(file, pe + 20)) + i * 40;
        let (va, size, raw) = (
            dword(file, s + 12),
            dword(file, s + 16),
            dword(file, s + 20),
        );
        image[va..va + size].copy_from_slice(&file[raw..raw + size]);
    }
    let export = dword(file, opt + if wide { 112 } else { 96 });
    let (names, ord, funcs) = (
        dword(image, export + 32),
        dword(image, export + 36),
        dword(image, export + 28),
    );
    for n in 0..dword(image, export + 24) {
        let name = dword(image, names + 4 * n);
        if image[name..].starts_with(b"dos_rs_bios_entry\0") {
            let entry = dword(image, funcs + 4 * usize::from(word(image, ord + 2 * n)));
            assert!(entry < image.len());
            return (BASE + entry, wide);
        }
    }
    panic!("BIOS entry missing")
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".start")]
pub extern "C" fn _start(info: &BiosInfo) -> ! {
    // E820 validated the staging, stack and destination ranges. They do not overlap.
    let (entry, wide) = unsafe {
        map(
            core::slice::from_raw_parts(info.staging as *const u8, info.len as usize),
            core::slice::from_raw_parts_mut(BASE as *mut u8, info.capacity as usize),
        )
    };
    unsafe {
        InitializeFloatingPointUnits();
        lgdt(&DescriptorTablePointer {
            base: GDT.as_ptr(),
            limit: (size_of::<[u64; 4]>() - 1) as u16,
        });
        if !wide {
            InternalSwitchStack(entry, 0, 0, STACK);
        }
        let tables = &mut *(&raw mut TABLES);
        tables.0.fill([0; 512]);
        tables.0[0][0] = tables.0[1].as_ptr() as u64 | 3;
        for i in 0..4 {
            tables.0[1][i] = tables.0[i + 2].as_ptr() as u64 | 3;
            for j in 0..512 {
                tables.0[i + 2][j] = (((i * 512 + j) as u64) << 21) | 0x83;
            }
        }
        x86::controlregs::cr3_write(ptr::addr_of!(TABLES) as u64);
        InternalX86EnablePaging64(24, entry as u64, 0, 0, STACK as u64)
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe {
        x86::io::outb(0xe9, b'E');
    }
    loop {
        core::hint::spin_loop()
    }
}
