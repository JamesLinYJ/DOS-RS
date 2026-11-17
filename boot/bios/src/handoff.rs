//! Fixed-width handoff from BIOS real mode to either PE kernel entry.
#[repr(C)]
pub struct BiosInfo {
    pub image_len: u32,
    pub image_capacity: u32,
    pub staging: u32,
    pub e820_address: u32,
    pub e820_count: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct E820MemoryRegion {
    pub start_addr: u64,
    pub len: u64,
    pub region_type: u32,
    pub acpi_extended_attributes: u32,
}

impl E820MemoryRegion {
    pub fn is_usable(&self) -> bool {
        // Donate ordinary enabled RAM. Persistent/slow memory, firmware error
        // logs and ranges with unknown extended attributes stay reserved.
        self.region_type == 1 && self.acpi_extended_attributes == 1
    }
}

// The real-mode producer and either kernel consumer must agree on the layout.
const _: () = {
    assert!(core::mem::size_of::<BiosInfo>() == 20);
    assert!(core::mem::size_of::<E820MemoryRegion>() == 24);
    assert!(core::mem::align_of::<E820MemoryRegion>() == 8);
};
