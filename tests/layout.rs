//! Host tests for the same no_std modules compiled into both kernels.
#![allow(dead_code)]
#[path = "../src/guest/arena.rs"]
mod arena;
#[path = "../src/kernel_cell.rs"]
mod kernel_cell;
#[path = "../src/guest/memory.rs"]
mod memory;
#[path = "../src/guest/mz.rs"]
mod mz;
#[path = "../src/guest/process.rs"]
mod process;
#[path = "../src/guest/state.rs"]
mod state;

use arena::{Arena, MemoryError};
use memory::CONVENTIONAL_END as END;
use memory::read_word;
use process::{CommandTail, Program};
use std::sync::Mutex;

const ROOT: u16 = 0x0b00;
const FLOOR: u16 = 0x0c00;
static RAM_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn placement_reserves_psp_environment_and_stack_for_every_process() {
    let mut arena = Arena::new();
    let (parent, end) = arena.process(ROOT, 3, 0x200, 0x200).unwrap();
    assert_eq!((parent, end), (ROOT, ROOT + 0x200));
    let heap = arena.alloc(parent, 32, 0).unwrap();
    assert_eq!(heap, end+1);
    let (child, _) = arena.process(ROOT, 3, 0x100, 0x100).unwrap();
    assert_eq!(child - 4, heap + 33);
    assert!(matches!(
        arena.free(child, heap),
        Err(MemoryError::InvalidBlock)
    ));
    arena.alloc(child, 0x111, 0).unwrap();
    arena.release_process(child);
    assert_eq!(arena.alloc(parent, 0x111, 0).unwrap(), heap + 33);
}

#[test]
fn free_and_resize_require_an_exact_owned_start() {
    let mut arena = Arena::new();
    let a = arena.alloc(ROOT, 8, 0).unwrap();
    let b = arena.alloc(ROOT, 12, 0).unwrap();
    assert!(matches!(
        arena.resize(ROOT, a, 9),
        Err(MemoryError::NoRoom(8))
    ));
    assert!(matches!(
        arena.free(ROOT, a + 1),
        Err(MemoryError::InvalidBlock)
    ));
    assert!(matches!(
        arena.resize(ROOT + 1, a, 2),
        Err(MemoryError::InvalidBlock)
    ));
    arena.free(ROOT, b).unwrap();
    assert!(matches!(
        arena.free(ROOT, b),
        Err(MemoryError::InvalidBlock)
    ));
    assert_eq!(arena.resize(ROOT, a, 18).unwrap(), a + 18);
    assert_eq!(arena.resize(ROOT, a, 1).unwrap(), a + 1);
    assert_eq!(arena.alloc(ROOT, 2, 0).unwrap(), a + 2);
}

#[test]
fn fragmented_arena_honours_all_three_fit_strategies() {
    let mut arena = Arena::new();
    let a = arena.alloc(ROOT, 20, 0).unwrap();
    arena.alloc(ROOT, 1, 0).unwrap();
    let b = arena.alloc(ROOT, 12, 0).unwrap();
    arena.alloc(ROOT, 1, 0).unwrap();
    arena.free(ROOT, a).unwrap();
    arena.free(ROOT, b).unwrap();
    let first = arena.alloc(ROOT, 10, 0).unwrap();
    assert_eq!(first, a);
    arena.free(ROOT, first).unwrap();
    assert_eq!(arena.alloc(ROOT, 10, 1).unwrap(), b);
    assert_eq!(arena.alloc(ROOT, 10, 2).unwrap(), END - 10);
}

#[test]
fn oversized_allocations_report_space_and_leave_it_available() {
    let mut arena = Arena::new();
    assert!(matches!(arena.alloc(ROOT, 0xffff, 0), Err(MemoryError::NoRoom(n)) if n == END-FLOOR));
    assert!(arena.process(END - 1, 2, 0x100, 0x100).is_err());
    assert_eq!(arena.alloc(ROOT, END - FLOOR, 0).unwrap(), FLOOR);
    assert!(matches!(
        arena.alloc(ROOT, 1, 0),
        Err(MemoryError::NoRoom(0))
    ));
}

fn mz_file() -> Vec<u8> {
    // One relocation at the second byte of a 4-byte module, followed by an overlay.
    let mut file = vec![0; 38];
    file[..2].copy_from_slice(b"MZ");
    for (at, val) in [
        (2, 36u16),
        (4, 1),
        (6, 1),
        (8, 2),
        (10, 2),
        (12, 2),
        (16, 32),
        (24, 28),
        (28, 1),
    ] {
        file[at..at + 2].copy_from_slice(&val.to_le_bytes());
    }
    file[32..].copy_from_slice(&[0xb8, 2, 0, 0xcb, 0xaa, 0xaa]);
    file
}

#[test]
fn mz_loads_declared_bytes_and_applies_relocations_once() {
    let _guard = RAM_LOCK.lock().unwrap();
    let ram = &mut *memory::storage();
    ram.clear();
    let file = mz_file();
    let mut arena = Arena::new();
    let (psp, cpu) = program(&file).load(ram, &mut arena).unwrap();
    let code = psp + 16;
    assert_eq!(cpu.selector(state::Segment::Code), code);
    assert_eq!(read_word(ram, code, 1), Some(code + 2));
    assert_eq!(ram.load(code, 4), Some(0));
    assert_eq!(read_word(ram, psp, 2), Some(code + 3));
}

fn program(image: &[u8]) -> Program<'_> {
    Program {
        name: b"TEST    EXE",
        image,
        preferred_psp: ROOT,
        com_paragraphs: 0x1000,
        environment: 0,
        parent: 0,
        tail: CommandTail::empty(),
    }
}

#[test]
fn invalid_images_never_modify_guest_ram_or_consume_a_block() {
    let _guard = RAM_LOCK.lock().unwrap();
    let ram = &mut *memory::storage();
    ram.clear();
    ram.store(ROOT, 0, 0xa5);
    let mut arena = Arena::new();
    for (at, value) in [(4, 0u16), (2, 512), (8, 1), (24, 31), (28, 3), (20, 4)] {
        let mut file = mz_file();
        file[at..at + 2].copy_from_slice(&value.to_le_bytes());
        assert!(matches!(program(&file).load(ram, &mut arena), Err(11)));
    }
    let file = mz_file();
    assert!(matches!(
        program(&file[..35]).load(ram, &mut arena),
        Err(11)
    ));
    assert!(matches!(
        program(&vec![0x90; 0x10000]).load(ram, &mut arena),
        Err(11)
    ));
    assert_eq!(ram.load(ROOT, 0), Some(0xa5));
    assert_eq!(program(&file).load(ram, &mut arena).unwrap().0, ROOT);
}

#[test]
fn maximum_command_tail_preserves_the_psp_boundary() {
    let _guard = RAM_LOCK.lock().unwrap();
    let ram = &mut *memory::storage();
    ram.clear();
    let mut arena = Arena::new();
    let mut executable = program(&[0xf4]);
    executable.tail = CommandTail::arguments(&[b'A'; 200]);
    let (psp, _) = executable.load(ram, &mut arena).unwrap();
    assert_eq!(ram.load(psp, 0x80), Some(126));
    assert_eq!(ram.load(psp, 0xff), Some(b'\r'));
    assert_eq!(ram.load(psp, 0x100), Some(0xf4));
}

#[path = "../src/dos/fat.rs"]
pub mod fat;
mod dos {
    pub use crate::fat;
    // Native allocation records extended errors through this adapter.
    pub mod ms_code {
        pub fn sys_ret_err(_: u16) {}
    }
}
mod bios {
    pub mod mscon {
        pub fn con_write_bytes(_: &[u8]) {}
    }
}
#[path = "../src/guest/disk.rs"]
mod disk;

#[test]
fn replacing_a_file_on_a_full_volume_preserves_the_original_on_failure() {
    disk::boot();
    let name = *b"ORIGINALDAT";
    assert!(disk::write_file(&name, b"original"));
    let mut serial = 0;
    while let Some((_, _, free, _)) = disk::allocation() {
        if free == 0 {
            break;
        }
        let filler = disk::canonical(format!("F{serial}.DAT").as_bytes()).unwrap();
        assert!(disk::write_file(
            &filler,
            &vec![0; usize::from(free.min(128)) * 512]
        ));
        serial += 1;
    }
    assert!(!disk::write_file(&name, &[42; 513]));
    let mut out = [0; 8];
    assert_eq!(disk::read_file(&name, &mut out), Some(8));
    assert_eq!(&out, b"original");
    assert!(disk::write_file(&name, b"replaced"));
    assert_eq!(disk::read_file(&name, &mut out), Some(8));
    assert_eq!(&out, b"replaced");
}

#[path = "../src/dos/alloc.rs"]
mod native_alloc;

#[test]
fn native_arena_can_split_to_its_last_header_and_rejoin_all_blocks() {
    let mut blocks = Vec::new();
    while let Ok(segment) = native_alloc::alloc(0) {
        blocks.push(segment);
    }
    assert_eq!(blocks.len(), 17);
    for segment in blocks.into_iter().rev() {
        assert_eq!(native_alloc::free(segment), 0);
        assert_eq!(native_alloc::free(segment), 9);
    }
    assert_eq!(native_alloc::largest_free(), 16);
    assert!(native_alloc::alloc(16).is_ok());
}

#[test]
fn kernel_storage_rejects_a_second_borrow_and_releases_on_unwind() {
    let cell = kernel_cell::KernelCell::new(0u16);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut one = cell.borrow();
        *one = 42;
        assert!(cell.try_borrow().is_none());
        panic!("abort a borrow");
    }));
    assert!(result.is_err());
    assert_eq!(*cell.borrow(), 42);
}
