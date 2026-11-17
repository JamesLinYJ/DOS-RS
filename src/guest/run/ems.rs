//! Windows EMM import support for interpreted DOS sessions.
include!("../../dos_memory/ems.rs");
include!("../../dos_memory/ems_import.rs");

impl Ems {
    pub(super) fn finish_import(import_map: &mut (usize, usize), ram: &mut GuestRam) {
        let (start, len) = core::mem::take(import_map);
        if len != 0 {
            ram.mark(start, len, false);
        }
    }
    pub(super) fn import(
        &self,
        cpu: &CpuState,
        ram: &mut GuestRam,
        import_map: &mut (usize, usize),
    ) -> Result<u16, u16> {
        self.import_at(
            cpu.selector(Segment::Data),
            cpu.wide(Slot::Data),
            cpu.wide(Slot::Count),
            0xf1200,
            ram,
            import_map,
        )
    }
}
