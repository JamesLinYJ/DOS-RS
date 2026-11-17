//! Native DOS request-packet entry for the resident LIM device.
include!("../../../src/dos_memory/ems.rs");
include!("../../../src/dos_memory/ems_import.rs");

impl Ems {
    pub(super) fn reserve_import(ram: &mut GuestRam) {
        // WIN386 obtains and locks the available XMS blocks before asking the
        // character device for its snapshot. Include every possible EMS page
        // here so publishing the snapshot never depends on free client memory.
        let header_bytes = (0x18dusize + 16 * HANDLES).next_multiple_of(1024);
        let page_bytes = (16 * ram.free_ems()).next_multiple_of(1024);
        if let Some(at) = ram.allocate_ext(header_bytes + page_bytes) {
            ram.import_header = Some(at);
            ram.import_pages = (at + header_bytes, page_bytes);
        }
    }
    fn guest_buffer(address: u32, bytes: u32) -> bool {
        // The V86 transfer must be representable even with the A20 gate open.
        if bytes == 0 || address + bytes > 0x10fff0 {
            return false;
        }
        (address / 4096..=(address + bytes - 1) / 4096)
            .all(|p| super::read32(super::FIRST + p * 4) & 7 == 7)
    }
    fn ioctl_read(&self, ram: &mut GuestRam, packet: u32) -> Result<u16, u16> {
        let off = super::word(packet + 14);
        let seg = super::word(packet + 16);
        let count = super::word(packet + 18);
        let at = u32::from(seg) * 16 + u32::from(off);
        if count != 6 || off > 0xfffa || !Self::guest_buffer(at, 6) || super::get(at) != 1 {
            return Err(3);
        }
        let base = ram.import_header.ok_or(12u16)?;
        let mut map = ram.import_pages;
        self.import_at(seg, off, count, base, ram, &mut map).map_err(|_| 12)
    }
    pub(super) fn device_command(&self, ram: &mut GuestRam, f: &super::Frame) {
        let packet = (f.ves << 4) + u32::from(f.bx as u16);
        if !Self::guest_buffer(packet, 5) {
            return;
        }
        let status = match super::get(packet + 2) {
            3 if super::get(packet) >= 20 && Self::guest_buffer(packet, 20) => {
                match self.ioctl_read(ram, packet) {
                    Ok(count) => {
                        super::put(packet + 18, count as u8);
                        super::put(packet + 19, (count >> 8) as u8);
                        0x100
                    }
                    Err(code) => 0x8100 | code,
                }
            }
            10 => 0x100,
            _ => 0x8103,
        };
        super::put(packet + 3, status as u8);
        super::put(packet + 4, (status >> 8) as u8);
    }
}
