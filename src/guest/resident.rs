//! Guest ABI for the built-in console and clock device headers.
pub(super) const SEGMENT: u16 = 0x0134;
pub(super) const CON: u16 = 0x100;
pub(super) const CLOCK: u16 = 0x112;
pub(super) const STRATEGY: u16 = 0x130;
pub(super) const CON_INTERRUPT: u16 = 0x131;
pub(super) const CLOCK_INTERRUPT: u16 = 0x132;
pub(super) fn pointer(offset: u16) -> [u8; 4] {
    [
        offset as u8,
        (offset >> 8) as u8,
        SEGMENT as u8,
        (SEGMENT >> 8) as u8,
    ]
}
pub(super) fn publish(ram: &mut super::memory::GuestRam) {
    for (offset, next, attributes, entry, name) in [
        (CON, pointer(CLOCK), 0x8003u16, CON_INTERRUPT, b"CON     "),
        (CLOCK, [255; 4], 0x8008, CLOCK_INTERRUPT, b"CLOCK$  "),
    ] {
        let mut header = [0; 18];
        header[..4].copy_from_slice(&next);
        header[4..6].copy_from_slice(&attributes.to_le_bytes());
        header[6..8].copy_from_slice(&STRATEGY.to_le_bytes());
        header[8..10].copy_from_slice(&entry.to_le_bytes());
        header[10..].copy_from_slice(name);
        for (i, v) in header.into_iter().enumerate() {
            ram.store(SEGMENT, offset + i as u16, v);
        }
    }
}
