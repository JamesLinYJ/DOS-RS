//! MZ load and relocation.
//!
//! The image is placed one paragraph block above the program segment prefix.
//! Each relocation word is increased by that load segment.

use super::memory::GuestRam;
use super::state::{CpuState, Segment, Slot};

pub fn load(image: &[u8], ram: &mut GuestRam, prefix: u16) -> Option<CpuState> {
    if image.len() < 28 || image[0] != b'M' || image[1] != b'Z' {
        return None;
    }
    let header_bytes = usize::from(word(image, 8)?) * 16;
    if header_bytes < 28 || header_bytes > image.len() {
        return None;
    }
    let reloc_count = word(image, 6)?;
    let reloc_at = usize::from(word(image, 24)?);
    let stack_disp = word(image, 14)?;
    let stack_pointer = word(image, 16)?;
    let instruction = word(image, 20)?;
    let code_disp = word(image, 22)?;
    let load_segment = prefix.wrapping_add(0x0010);
    let mut offset = 0u16;
    for byte in image.get(header_bytes..)? {
        if !ram.store(load_segment, offset, *byte) {
            return None;
        }
        offset = offset.wrapping_add(1);
    }
    let mut index = 0u16;
    while index < reloc_count {
        let at = reloc_at + usize::from(index) * 4;
        let place = word(image, at)?;
        let place_segment = load_segment.wrapping_add(word(image, at + 2)?);
        let low = ram.load(place_segment, place)?;
        let high = ram.load(place_segment, place.wrapping_add(1))?;
        let fixed = u16::from_le_bytes([low, high]).wrapping_add(load_segment);
        let bytes = fixed.to_le_bytes();
        if !ram.store(place_segment, place, bytes[0])
            || !ram.store(place_segment, place.wrapping_add(1), bytes[1])
        {
            return None;
        }
        index += 1;
    }
    if !ram.store(prefix, 0, 0xCD) || !ram.store(prefix, 1, 0x20) {
        return None;
    }
    let limit = 0x2000u16.to_le_bytes();
    if !ram.store(prefix, 2, limit[0]) || !ram.store(prefix, 3, limit[1]) {
        return None;
    }
    if !plant_environment(ram, prefix) {
        return None;
    }
    let mut cpu = CpuState::at_com(prefix);
    cpu.set_instruction(instruction);
    cpu.set_selector(Segment::Code, load_segment.wrapping_add(code_disp));
    cpu.set_selector(Segment::Stack, load_segment.wrapping_add(stack_disp));
    cpu.set_wide(Slot::Stack, stack_pointer);
    Some(cpu)
}

fn plant_environment(ram: &mut GuestRam, prefix: u16) -> bool {
    const ENV_SEGMENT: u16 = 0x0600;
    let segment = ENV_SEGMENT.to_le_bytes();
    if !ram.store(prefix, 0x2C, segment[0]) || !ram.store(prefix, 0x2D, segment[1]) {
        return false;
    }
    if !ram.store(prefix, 0x80, 1) || !ram.store(prefix, 0x81, b'\r') {
        return false;
    }
    let text = b"\0\x01\0C:\\GUEST.EXE\0";
    let mut offset = 0u16;
    for byte in text {
        if !ram.store(ENV_SEGMENT, offset, *byte) {
            return false;
        }
        offset = offset.wrapping_add(1);
    }
    true
}

fn word(image: &[u8], at: usize) -> Option<u16> {
    let bytes = image.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}
