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
