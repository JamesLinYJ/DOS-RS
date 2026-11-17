//! Windows EMM import support for interpreted DOS sessions.
include!("../../dos_memory/ems.rs");

impl Ems {
    pub(super) fn finish_import(import_map: &mut (usize, usize), ram: &mut GuestRam) {
        let (start, len) = core::mem::take(import_map);
        if len != 0 {
            ram.mark(start, len, false);
        }
    }
    /// Global EMM Import Specification 1.0. Windows takes a snapshot of the
    /// hardware frame and the live LIM handles before its VMM takes ownership.
    pub(super) fn import(
        &self,
        cpu: &CpuState,
        ram: &mut GuestRam,
        import_map: &mut (usize, usize),
    ) -> Result<u16, u16> {
        let out = Far {
            seg: cpu.selector(Segment::Data),
            off: cpu.wide(Slot::Data),
        };
        if cpu.wide(Slot::Count) != 6 || out.byte(ram, 0) != 1 {
            return Err(1);
        }
        let base = 0xf1200usize;
        let count = self.handles.iter().flatten().count();
        let size = 0x18d + 16 * count;
        let pages = self
            .handles
            .iter()
            .flatten()
            .map(|h| usize::from(h.pages))
            .sum::<usize>();
        let need = (pages * 16).div_ceil(1024) * 1024;
        if need > import_map.1 {
            let at = ram.allocate_ext(need).ok_or(8u16)?;
            if import_map.1 != 0 {
                ram.mark(import_map.0, import_map.1, false);
            }
            *import_map = (at, need);
        }
        let mut storage = [0; 0x18d + 16 * HANDLES];
        let block = &mut storage[..size];
        block[0] = 4;
        block[2..4].copy_from_slice(&(size as u16).to_le_bytes());
        block[4] = 1;
        for (i, frame) in block[10..0x18a].chunks_exact_mut(6).enumerate() {
            if let Some(slot) = i.checked_sub(EMS_FRAME / EMS_PAGE).filter(|n| *n < 4) {
                let (handle, page) = self.map[slot];
                frame.copy_from_slice(&[3, handle as u8, 0, 0, slot as u8, 0]);
                frame[2..4]
                    .copy_from_slice(&(if page == EMPTY { 0x7fff } else { page }).to_le_bytes());
            } else {
                frame.copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xaa]);
            }
        }
        block[0x18a] = 16; // four hardware slots, three entries per slot plus four
        block[0x18c] = count as u8;
        let mut map = import_map.0;
        for (rec, (id, handle)) in block[0x18d..].chunks_exact_mut(16).zip(
            self.handles
                .iter()
                .enumerate()
                .filter_map(|(i, h)| h.map(|v| (i, v))),
        ) {
            rec[0] = id as u8;
            rec[2..10].copy_from_slice(&handle.name);
            rec[10..12].copy_from_slice(&handle.pages.to_le_bytes());
            if handle.pages != 0 {
                rec[12..16].copy_from_slice(&(map as u32).to_le_bytes());
            }
            for page in 0..handle.pages {
                let address = self
                    .physical((id as u16, page))
                    .map_err(|_| 13u16)?
                    .ok_or(13u16)?;
                for offset in [0, 4096, 8192, 12288] {
                    let pte = (address + offset) as u32 | 7;
                    for byte in pte.to_le_bytes() {
                        ram.set_bus_byte(map, byte);
                        map += 1;
                    }
                }
            }
        }
        for (i, &b) in block.iter().enumerate() {
            ram.set_bus_byte(base + i, b);
        }
        out.put(ram, 0, &(base as u32).to_le_bytes());
        out.put(ram, 4, &[1, 0]);
        Ok(6)
    }
}
