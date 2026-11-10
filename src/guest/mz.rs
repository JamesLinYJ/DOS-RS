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
    ram.release_image();
    let load_segment = prefix.wrapping_add(0x0010);
    let payload = image.get(header_bytes..)?;
    for (placed, byte) in payload.iter().enumerate() {
        let segment = load_segment.wrapping_add((placed >> 4) as u16);
        let offset = (placed & 0xF) as u16;
        if !ram.store(segment, offset, *byte) {
            return None;
        }
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
    let limit = super::memory::CONVENTIONAL_END.to_le_bytes();
    if !ram.store(prefix, 2, limit[0]) || !ram.store(prefix, 3, limit[1]) {
        return None;
    }
    if !plant_environment(ram, prefix, b"GUEST   EXE") {
        return None;
    }
    let mut cpu = CpuState::at_com(prefix);
    cpu.set_instruction(instruction);
    cpu.set_selector(Segment::Code, load_segment.wrapping_add(code_disp));
    cpu.set_selector(Segment::Stack, load_segment.wrapping_add(stack_disp));
    cpu.set_wide(Slot::Stack, stack_pointer);
    Some(cpu)
}

pub(super) fn plant_environment(ram: &mut GuestRam, prefix: u16, program: &[u8; 11]) -> bool {
    const ENV_SEGMENT: u16 = 0x0600;
    let segment = ENV_SEGMENT.to_le_bytes();
    if !ram.store(prefix, 0x2C, segment[0]) || !ram.store(prefix, 0x2D, segment[1]) {
        return false;
    }
    if !ram.store(prefix, 0x80, 1) || !ram.store(prefix, 0x81, b'\r') {
        return false;
    }
    let mut text = [0u8; 40];
    let mut length = 0usize;
    for byte in b"PATH=\\\0\0\x01\0C:\\" {
        text[length] = *byte;
        length += 1;
    }
    let mut index = 0usize;
    while index < 8 && program[index] != b' ' {
        text[length] = program[index];
        length += 1;
        index += 1;
    }
    text[length] = b'.';
    length += 1;
    text[length] = program[8];
    text[length + 1] = program[9];
    text[length + 2] = program[10];
    length += 3;
    text[length] = 0;
    let mut offset = 0u16;
    while usize::from(offset) <= length {
        if !ram.store(ENV_SEGMENT, offset, text[usize::from(offset)]) {
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
