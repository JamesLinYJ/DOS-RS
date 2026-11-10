//! Checked guest RAM.
//!
//! A real-mode address is `segment * 16 + offset`. The twentieth address bit
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

static mut STORAGE: GuestRam = GuestRam {
    bytes: [0; RAM_BYTES],
};

pub fn storage() -> &'static mut GuestRam {
    unsafe { &mut *core::ptr::addr_of_mut!(STORAGE) }
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
        let linear = real_linear(segment, offset)? as usize;
        self.bytes.get(linear).copied()
    }

    pub fn store(&mut self, segment: u16, offset: u16, value: u8) -> bool {
        let Some(linear) = real_linear(segment, offset) else {
            return false;
        };
        if let Some(slot) = self.bytes.get_mut(linear as usize) {
            *slot = value;
            true
        } else {
            false
        }
    }
}
