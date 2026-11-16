// The DOS and host builds use the same editor core with different storage.
#[path = "../native-tools/src/editor/gap_buffer.rs"]
mod gap_buffer;

use gap_buffer::{GapBuffer, Storage};

struct Memory(Vec<u8>);

impl Storage for Memory {
    fn capacity(&self) -> usize {
        self.0.len()
    }
    fn byte(&self, at: usize) -> u8 {
        self.0[at]
    }
    fn write(&mut self, at: usize, value: u8) {
        self.0[at] = value;
    }
    fn move_bytes(&mut self, from: usize, to: usize, count: usize) {
        self.0.copy_within(from..from + count, to);
    }
}

#[test]
fn edits_across_the_segment_boundary_on_a_wide_target() {
    let original = vec![b'a'; 130_000];
    let mut storage = original.clone();
    storage.resize(original.len() + 8, 0);
    let mut doc = GapBuffer::open(Memory(storage), original.len());
    assert_eq!(doc.capacity, 130_008);
    assert!(doc.replace(65_535, 2, b"hello", 5));
    assert_eq!(doc.byte(65_536), b'e');
    assert!(doc.replace(42, 0, b"X", 1));
    let mut expected = original;
    expected.splice(65_535..65_537, *b"hello");
    expected.insert(42, b'X');
    assert_eq!(&doc.flatten().0[..expected.len()], expected);
}
