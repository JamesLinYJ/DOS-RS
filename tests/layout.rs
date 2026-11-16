//! Host tests for the same no_std modules compiled into both kernels.
#![allow(dead_code)]
extern crate alloc;
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

use arena::UMB_ROOT as END;
use arena::{Arena, MemoryError};
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
    assert_eq!(heap, end + 1);
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
fn linked_upper_regions_reserve_the_ems_frame_and_survive_unlink() {
    let mut arena = Arena::new();
    let (driver, room) = arena.driver(true, 40).unwrap();
    assert_eq!((driver, room), (0xe001, 0xfff));
    arena.upper_linked = true;
    let xms = arena.alloc(arena::XMS_OWNER, 0x7fe, 0x40).unwrap();
    assert_eq!(xms, 0xc801);
    assert!(matches!(
        arena.alloc(ROOT, 1, 0x40),
        Err(MemoryError::NoRoom(0))
    ));
    let fallback = arena.alloc(ROOT, 1, 0x80).unwrap();
    assert_eq!(fallback, FLOOR);
    arena.upper_linked = false;
    assert!(matches!(
        arena.free(ROOT, xms),
        Err(MemoryError::InvalidBlock)
    ));
    arena.free(arena::XMS_OWNER, xms).unwrap();
    arena.free(8, driver).unwrap();
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
        directory: b"A:\\",
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

mod dos {
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
#[path = "../src/vfs.rs"]
mod vfs;
use vfs::dos as disk;

static DISK_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn replacing_a_file_on_a_full_volume_preserves_the_original_on_failure() {
    let _guard = DISK_LOCK.lock().unwrap();
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

#[test]
fn open_handles_follow_renames_and_reclaim_unlinked_chains_on_final_close() {
    let _guard = DISK_LOCK.lock().unwrap();
    disk::boot();
    let name = *b"PIN     DAT";
    let renamed = *b"MOVED   DAT";
    let free = || disk::space().unwrap().2;
    let initial = free();
    let data = [7; 1537];
    assert!(disk::write_file(&name, &data));
    let a = disk::OpenFile::open(0, &name, true).unwrap();
    let b = disk::OpenFile::open(0, &name, false).unwrap();
    assert_eq!(a.identity(), b.identity());
    let linked = free();
    assert!(disk::rename_file(&name, &renamed));
    assert!(disk::remove_file(&renamed));
    assert_eq!(free(), linked);
    assert!(matches!(
        disk::OpenFile::open(0, &renamed, false),
        Err(disk::FileError::Missing)
    ));
    assert!(disk::write_file(&name, b"new"));
    let new = disk::OpenFile::open(0, &name, false).unwrap();
    assert_ne!(a.identity(), new.identity());
    a.set_stamp(123, 456);
    assert_eq!(b.stamp(), (123, 456));
    assert_ne!(new.stamp(), b.stamp());
    a.write(1536, b"end").unwrap();
    drop(a);
    let mut tail = [0; 4];
    assert_eq!(b.read(1535, &mut tail), Ok(4));
    assert_eq!(&tail, b"\x07end");
    drop(b);
    drop(new);
    assert!(disk::remove_file(&name));
    assert_eq!(free(), initial);
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

#[test]
fn color_status_reports_display_blank_and_vertical_retrace() {
    let _guard = RAM_LOCK.lock().unwrap();
    let mut ram = memory::storage();
    ram.clear();
    let mut seen = [false; 10];
    for _ in 0..45000 {
        let status = ram.system_port(0x3da, false, 0).unwrap();
        assert_eq!(status & !9, 0);
        seen[usize::from(status)] = true;
    }
    assert!(seen[0] && seen[1] && seen[9]);
}

#[test]
fn zero_allocation_mz_relocates_at_top_of_the_largest_process_block() {
    let _guard = RAM_LOCK.lock().unwrap();
    let mut ram = memory::storage();
    ram.clear();
    let mut arena = Arena::new();
    arena.process(ROOT + 0x120, 2, 0x100, 0x100).unwrap();
    let mut file = mz_file();
    file[10..14].fill(0);
    file[16..18].copy_from_slice(&16u16.to_le_bytes());
    let (psp, cpu) = program(&file).load(&mut ram, &mut arena).unwrap();
    let code = cpu.selector(state::Segment::Code);
    assert!(psp > ROOT + 0x120);
    assert_eq!(read_word(&ram, psp, 2), Some(END));
    assert_eq!(code, END - 1);
    assert_eq!(read_word(&ram, code, 1), Some(code + 2));
    assert_eq!(cpu.selector(state::Segment::Extra), psp);
}

#[test]
fn flat_programs_leave_allocation_room_and_renamed_windows_loader_gets_the_largest_block() {
    let _guard = RAM_LOCK.lock().unwrap();
    let ram = &mut *memory::storage();
    ram.clear();
    let mut arena = Arena::new();
    let (psp, _) = program(&[0xc3]).load(ram, &mut arena).unwrap();
    assert_eq!(read_word(ram, psp, 2), Some(psp + 0x1000));
    assert!(arena.alloc(psp, 32, 0).is_ok());
    let mut loader = [0u8; 32];
    loader[3..5].copy_from_slice(b"XW");
    loader[19..27].copy_from_slice(b"SMARTAAR");
    let mut arena = Arena::new();
    let (psp, _) = program(&loader).load(ram, &mut arena).unwrap();
    assert_eq!(read_word(ram, psp, 2), Some(END));
}

#[test]
fn fcb_keeps_its_object_and_respects_the_end_of_the_dta_segment() {
    let _guard = DISK_LOCK.lock().unwrap();
    disk::boot();
    let name = *b"RECORDS BIN";
    let old = (0u8..=255).chain(0..17).collect::<Vec<_>>();
    assert!(disk::write_file(&name, &old));
    let mut raw = [0; 37];
    raw[0] = 2; // Address the B alias explicitly while A is the default.
    raw[1..12].copy_from_slice(&name);
    let mut table = vfs::fcb::Table::new();
    table.open(7, &mut raw, false).unwrap();
    assert_eq!(
        table.transfer(7, &mut raw, 0x14, 1, 127, |_, _, _| panic!()),
        (2, 0)
    );
    assert!(disk::remove_file(&name));
    assert!(disk::write_file(&name, b"replacement"));
    let mut buf = [0xff; 384];
    let status = table.transfer(7, &mut raw, 0x27, 3, buf.len(), |at, record, input| {
        assert!(!input);
        buf[at..at + record.len()].copy_from_slice(record);
        true
    });
    assert_eq!(status, (3, 3));
    assert_eq!(&buf[..old.len()], old.as_slice());
    assert!(buf[old.len()..].iter().all(|&b| b == 0));
    table.close(7, &mut raw).unwrap();
    let mut actual = [0; 11];
    assert_eq!(disk::read_file(&name, &mut actual), Some(11));
    assert_eq!(&actual, b"replacement");
}

#[test]
fn partition_errors_do_not_turn_a_protected_disk_into_a_raw_volume() {
    use dosrs_block::{BlockRead, Error, partition::data_partitions};
    struct Disk([u8; 1024]);
    impl BlockRead for Disk {
        fn read_at(&self, pos: u64, out: &mut [u8]) -> Result<(), Error> {
            let start = pos as usize;
            out.copy_from_slice(self.0.get(start..start + out.len()).ok_or(Error)?);
            Ok(())
        }
        fn size_bytes(&self) -> u64 {
            1024 * 512
        }
    }
    let mut disk = Disk([0; 1024]);
    disk.0[510..512].copy_from_slice(&[0x55, 0xaa]);
    assert!(data_partitions(&disk).unwrap().is_none());
    let mut partition = |slot: usize, start: u32, count: u32| {
        let e = &mut disk.0[446 + 16 * slot..462 + 16 * slot];
        e[4] = 7;
        e[8..12].copy_from_slice(&start.to_le_bytes());
        e[12..16].copy_from_slice(&count.to_le_bytes());
    };
    partition(0, 100, 200);
    partition(1, 250, 50);
    assert!(data_partitions(&disk).is_err());
    disk.0[462..478].fill(0);
    let selected = data_partitions(&disk).unwrap().unwrap();
    assert_eq!((selected[0].first, selected[0].sectors), (100, 200));
    disk.0[450] = 0xee;
    disk.0[454..458].copy_from_slice(&1u32.to_le_bytes());
    assert!(data_partitions(&disk).is_err());
}

#[test]
fn a_volume_cannot_write_through_its_read_only_view_or_across_partition_edges() {
    use dosrs_block::{BlockDevice, BlockRead, BlockWindow, Error, partition::Partition};
    use std::sync::Arc;
    struct Memory(Mutex<[u8; 1536]>);
    impl BlockRead for Memory {
        fn size_bytes(&self) -> u64 {
            1536
        }
        fn read_at(&self, offset: u64, out: &mut [u8]) -> Result<(), Error> {
            let start = offset as usize;
            out.copy_from_slice(&self.0.lock().unwrap()[start..start + out.len()]);
            Ok(())
        }
    }
    impl BlockDevice for Memory {
        fn is_writable(&self) -> bool {
            true
        }
        fn write_at(&self, offset: u64, data: &[u8]) -> Result<(), Error> {
            let start = offset as usize;
            self.0.lock().unwrap()[start..start + data.len()].copy_from_slice(data);
            Ok(())
        }
        fn flush(&self) -> Result<(), Error> {
            Ok(())
        }
    }
    let disk = Arc::new(Memory(Mutex::new([0; 1536])));
    let window = BlockWindow::disk(disk.clone())
        .partition(Partition {
            first: 1,
            sectors: 1,
            read_only: false,
        })
        .unwrap();
    window.write_at(511, &[0xf1]).unwrap();
    assert!(window.write_at(511, &[0xf2, 0xf3]).is_err());
    assert!(window.read_at(u64::MAX, &mut [0; 1]).is_err());
    let protected = window.read_only();
    assert!(!protected.is_writable());
    assert!(protected.write_at(511, &[0xf4]).is_err());
    let mut tail = [0; 3];
    disk.read_at(1022, &mut tail).unwrap();
    assert_eq!(tail, [0, 0xf1, 0]);
}
