//! BIOS EDD reads and E820 memory validation before protected-mode entry.
#![no_std]
#![no_main]
extern crate self as bootloader_x86_64_bios_common;
#[repr(C)]
pub struct BiosInfo {
    image_len: u32,
    image_capacity: u32,
    staging: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct E820MemoryRegion {
    start_addr: u64,
    len: u64,
    region_type: u32,
    acpi_extended_attributes: u32,
}
pub mod racy_cell {
    include!(concat!(env!("BIOS_CPU"), "/bios/common/src/racy_cell.rs"));
}
mod dap {
    include!(concat!(env!("BIOS_CPU"), "/bios/stage-2/src/dap.rs"));
}
mod mode {
    include!(concat!(
        env!("BIOS_CPU"),
        "/bios/stage-2/src/protected_mode.rs"
    ));
}
mod e820 {
    include!(concat!(env!("BIOS_CPU"), "/bios/stage-2/src/memory_map.rs"));
}
fn split_array_ref<const N: usize, T>(slice: &[T]) -> (&[T; N], &[T]) {
    slice.split_first_chunk().unwrap()
}
const SECTOR: usize = 512;
#[repr(align(16))]
struct Buffer([u8; 4096]);
static mut BUFFER: Buffer = Buffer([0; 4096]);

fn read(drive: u16, lba: u32, count: u16, buffer: &mut Buffer) {
    let addr = buffer.0.as_mut_ptr() as usize;
    // EDD writes up to eight sectors into the uniquely borrowed low-memory buffer.
    unsafe {
        dap::DiskAddressPacket::from_lba(lba.into(), count, (addr & 15) as u16, (addr >> 4) as u16)
            .perform_load(drive);
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".start")]
pub extern "C" fn _start(drive: u16, table: *const u8) -> ! {
    mode::enter_unreal_mode();
    // Partition two is the bounded DOS-RS payload written by the image builder.
    let part = unsafe { core::slice::from_raw_parts(table.add(16), 16) };
    assert_eq!(part[4], 0xda);
    let mut lba = u32::from_le_bytes(part[8..12].try_into().unwrap());
    let sectors = u32::from_le_bytes(part[12..16].try_into().unwrap());
    let buffer = unsafe { &mut *(&raw mut BUFFER) };
    read(drive, lba, 1, buffer);
    assert_eq!(&buffer.0[..8], b"DOSRSBI1");
    let field = |n: usize| u32::from_le_bytes(buffer.0[n..n + 4].try_into().unwrap());
    let (stub, file, capacity) = (field(8), field(12), field(16));
    assert!(
        stub > 0 && stub < 0x100000 && file > 0 && capacity > file && capacity <= i32::MAX as u32
    );
    assert_eq!(1 + stub.div_ceil(512) + file.div_ceil(512), sectors);
    // Keep the on-disk PE outside the destination that the mapper clears.
    let staging = 0x01000000u32
        .checked_add(capacity)
        .unwrap()
        .next_multiple_of(4096);
    let end = staging.checked_add(file.next_multiple_of(512)).unwrap();
    let memory = unsafe { e820::query_memory_map() }.unwrap();
    assert!(memory.iter().any(|r| r.region_type == 1
        && r.start_addr <= 0x100000
        && r.start_addr.saturating_add(r.len) >= u64::from(end)));
    lba += 1;
    for (start, bytes) in [(0x100000u32, stub), (staging, file)] {
        let mut left = bytes.div_ceil(512);
        let mut addr = start;
        while left > 0 {
            let n = left.min(8);
            read(drive, lba, n as u16, buffer);
            unsafe {
                mode::copy_to_protected_mode(addr as *mut u8, &buffer.0[..n as usize * SECTOR]);
            }
            lba += n;
            addr += 512 * n;
            left -= n;
        }
    }
    let mut info = BiosInfo {
        image_len: file,
        image_capacity: capacity,
        staging,
    };
    mode::enter_protected_mode_and_jump_to_stage_3(0x100000 as *const u8, &mut info);
    fail(b'R')
}
#[unsafe(no_mangle)]
extern "C" fn fail(code: u8) -> ! {
    unsafe {
        x86::io::outb(0xe9, code);
    }
    loop {
        core::hint::spin_loop()
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    fail(b'P')
}
