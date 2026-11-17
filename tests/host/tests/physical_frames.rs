#[path = "../../../src/arch/frames/bitmap.rs"]
mod bitmap;

use bitmap::FreeFrames;

#[test]
fn only_complete_usable_frames_and_reservations_take_precedence() {
    let mut memory = FreeFrames::new();
    memory.region(0x1003, 0x7000, true);
    memory.region(0x3000, 0x2000, true); // Duplicate usable ranges.
    memory.region(0x3fff, 2, false); // A hole straddles two physical pages.
    memory.region(0x100_0000_0000, u64::MAX, true);
    memory.region(0xffff_ffff_ffff_ff00, u64::MAX, true);
    memory.region(0xfffe, 1, true);
    assert_eq!(memory.total, 4);
    let frames: Vec<_> = std::iter::from_fn(|| memory.take()).collect();
    assert_eq!(frames, [0x2000, 0x5000, 0x6000, 0x7000]);
    assert_eq!(memory.free, 0);
    memory.release(0x6000);
    assert_eq!(memory.free, 1);
    assert_eq!(memory.take(), Some(0x6000));
    assert_eq!(memory.take(), None);
}

#[test]
fn last_directly_mapped_frame_can_be_donated_and_recycled() {
    let mut memory = FreeFrames::new();
    memory.region(0xffff_f000, 0x2000, true);
    assert_eq!(memory.take(), Some(0xffff_f000));
    assert_eq!(memory.take(), None);
    memory.release(0xffff_f000);
    assert_eq!((memory.total, memory.free), (1, 1));
}

#[path = "../../../boot/bios/src/handoff.rs"]
mod handoff;
use handoff::E820MemoryRegion;
#[path = "../../../boot/bios/src/e820_record.rs"]
mod e820_record;

#[test]
fn legacy_e820_records_are_enabled_but_disabled_extended_records_are_ignored() {
    let mut bytes = e820_record::buffer();
    assert_eq!(bytes[20], 1);
    bytes[..8].copy_from_slice(&0x200000_u64.to_le_bytes());
    bytes[8..16].copy_from_slice(&0x100000_u64.to_le_bytes());
    bytes[16..20].copy_from_slice(&1_u32.to_le_bytes());
    bytes[20] = 0;
    let legacy = e820_record::decode(&bytes, 20).unwrap().unwrap();
    assert_eq!(
        (
            legacy.start_addr,
            legacy.len,
            legacy.region_type,
            legacy.acpi_extended_attributes
        ),
        (0x200000, 0x100000, 1, 1)
    );
    assert!(e820_record::decode(&bytes, 24).unwrap().is_none());
    bytes[20] = 2;
    assert!(e820_record::decode(&bytes, 24).unwrap().is_none());
    bytes[20] = 3;
    assert_eq!(
        e820_record::decode(&bytes, 24)
            .unwrap()
            .unwrap()
            .acpi_extended_attributes,
        3
    );
    for invalid in [1, 19, 21, 23, 25] {
        assert!(e820_record::decode(&bytes, invalid).is_err());
    }
}

#[test]
fn bios_staging_checks_adjacent_ram_and_reserved_overlaps() {
    let region = |start_addr, len, region_type, acpi_extended_attributes| E820MemoryRegion {
        start_addr,
        len,
        region_type,
        acpi_extended_attributes,
    };
    let memory = [region(16, 16, 1, 1), region(0, 16, 1, 1)];
    assert!(e820_record::can_stage(&memory, 0, 32));
    assert!(!e820_record::can_stage(&memory, 0, 33));
    for hole in [
        region(12, 2, 2, 1),
        region(14, 3, 1, 3),
        region(18, 2, 1, 9),
    ] {
        assert!(!e820_record::can_stage(
            &[memory[0], memory[1], hole],
            0,
            32
        ));
    }
}
