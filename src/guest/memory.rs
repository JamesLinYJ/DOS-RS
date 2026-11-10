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
    /// Data view of an allocation that aliases the program image.
    /// Instruction bytes stay in `bytes` so the program can keep running.
    held: [u8; 16384],
    held_on: bool,
    held_at: u32,
    held_len: u32,
}

static mut STORAGE: GuestRam = GuestRam {
    bytes: [0; RAM_BYTES],
    held: [0; 16384],
    held_on: false,
    held_at: 0,
    held_len: 0,
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
        self.release_image();
        // Microsoft C walks a message list at FFFF:0758. That address folds
        // to 0748 here. A word of all ones is the empty list, so the walk ends.
        self.bytes[0x748] = 0xFF;
        self.bytes[0x749] = 0xFF;
    }

    pub fn release_image(&mut self) {
        self.held_on = false;
        self.held_len = 0;
    }

    /// Keep later data writes to this block off the instruction bytes.
    pub fn hold_image(&mut self, segment: u16, paragraphs: u16) {
        let Some(start) = real_linear(segment, 0) else {
            return;
        };
        let mut length = u32::from(paragraphs).saturating_mul(16);
        if length > self.held.len() as u32 {
            length = self.held.len() as u32;
        }
        let mut index = 0u32;
        while index < length {
            let linear = (start + index) as usize;
            self.held[index as usize] = self.bytes.get(linear).copied().unwrap_or(0);
            index += 1;
        }
        self.held_at = start;
        self.held_len = length;
        self.held_on = length != 0;
    }

    pub fn load(&self, segment: u16, offset: u16) -> Option<u8> {
        let linear = real_linear(segment, offset)?;
        if let Some(byte) = self.held_byte(linear) {
            return Some(byte);
        }
        self.bytes.get(linear as usize).copied()
    }

    /// Instruction byte. Ignores the data view of an aliased allocation.
    pub fn load_code(&self, segment: u16, offset: u16) -> Option<u8> {
        let linear = real_linear(segment, offset)? as usize;
        self.bytes.get(linear).copied()
    }

    fn held_byte(&self, linear: u32) -> Option<u8> {
        if !self.held_on || linear < self.held_at {
            return None;
        }
        let index = linear - self.held_at;
        if index >= self.held_len {
            return None;
        }
        Some(self.held[index as usize])
    }

    pub fn store(&mut self, segment: u16, offset: u16, value: u8) -> bool {
        let Some(linear) = real_linear(segment, offset) else {
            return false;
        };
        if self.held_on && linear >= self.held_at {
            let index = linear - self.held_at;
            if index < self.held_len {
                self.held[index as usize] = value;
                return true;
            }
        }
        if let Some(slot) = self.bytes.get_mut(linear as usize) {
            *slot = value;
            true
        } else {
            false
        }
    }
}
