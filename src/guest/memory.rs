//! Checked guest RAM.
//!
//! A real-mode address is `segment * 16 + offset`. Address line A20
//! is not driven, so the sum is folded into the first megabyte. Conventional
//! memory is the first 640KiB of that map. The byte must sit inside this
//! buffer. The buffer is not a host pointer handed to guest code.

/// One megabyte, the real-mode space with the high address bit folded in.
pub const RAM_BYTES: usize = 0x100000;

/// First paragraph past conventional memory. 640KiB.
pub const CONVENTIONAL_END: u16 = 0xA000;

pub struct GuestRam {
    bytes: [u8; RAM_BYTES],
}

static STORAGE: [crate::kernel_cell::KernelCell<GuestRam>; 4] = [const {
    crate::kernel_cell::KernelCell::new(GuestRam {
        bytes: [0; RAM_BYTES],
    })
}; 4];

pub(crate) fn storage() -> crate::kernel_cell::Guard<'static, GuestRam> {
    session_ram(0)
}
pub(crate) fn session_ram(id: usize) -> crate::kernel_cell::Guard<'static, GuestRam> {
    STORAGE[id].borrow()
}

pub fn real_linear(segment: u16, offset: u16) -> Option<u32> {
    let sum = u32::from(segment)
        .checked_mul(16)?
        .checked_add(u32::from(offset))?;
    Some(sum & 0x000F_FFFF)
}

impl GuestRam {
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        // Microsoft C walks a message list at FFFF:0758. That address folds
        // to 0748 here. A word of all ones is the empty list, so the walk ends.
        self.bytes[0x748] = 0xFF;
        self.bytes[0x749] = 0xFF;
    }

    pub fn load(&self, segment: u16, offset: u16) -> Option<u8> {
        self.bytes
            .get(real_linear(segment, offset)? as usize)
            .copied()
    }

    pub fn store(&mut self, segment: u16, offset: u16, value: u8) -> bool {
        let Some(linear) = real_linear(segment, offset) else {
            return false;
        };
        self.bytes[linear as usize] = value;
        true
    }

    pub(super) fn paragraphs_mut(&mut self, start: u16, end: u16) -> &mut [u8] {
        &mut self.bytes[usize::from(start) * 16..usize::from(end) * 16]
    }
}

pub(super) fn read_word(ram: &GuestRam, segment: u16, offset: u16) -> Option<u16> {
    let low = ram.load(segment, offset)?;
    let high = ram.load(segment, offset.wrapping_add(1))?;
    Some(u16::from_le_bytes([low, high]))
}
