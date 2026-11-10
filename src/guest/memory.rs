//! Checked guest RAM.
//!
//! A real-mode address is `segment * 16 + offset`. The add must fit in 32
//! bits. The byte must also sit inside this buffer. The buffer is not a host
//! pointer handed to guest code.

pub const RAM_BYTES: usize = 0x20000;

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
    u32::from(segment)
        .checked_mul(16)?
        .checked_add(u32::from(offset))
}

impl GuestRam {
    pub fn clear(&mut self) {
        self.bytes.fill(0);
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
