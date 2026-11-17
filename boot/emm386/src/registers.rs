//! DOS register views over the hardware-created virtual-8086 interrupt frame.
use super::Frame;
#[derive(Clone, Copy)]
pub enum Slot {
    Accumulator,
    Base,
    Count,
    Data,
    Source,
    Dest,
    Stack,
}
#[derive(Clone, Copy)]
pub enum Segment {
    Code,
    Data,
    Extra,
    Stack,
}
impl Frame {
    pub fn dword(&self, k: Slot) -> u32 {
        match k {
            Slot::Accumulator => self.ax,
            Slot::Base => self.bx,
            Slot::Count => self.cx,
            Slot::Data => self.dx,
            Slot::Source => self.si,
            Slot::Dest => self.di,
            Slot::Stack => self.sp,
        }
    }
    pub fn set_dword(&mut self, k: Slot, v: u32) {
        *match k {
            Slot::Accumulator => &mut self.ax,
            Slot::Base => &mut self.bx,
            Slot::Count => &mut self.cx,
            Slot::Data => &mut self.dx,
            Slot::Source => &mut self.si,
            Slot::Dest => &mut self.di,
            Slot::Stack => &mut self.sp,
        } = v
    }
    pub fn wide(&self, k: Slot) -> u16 {
        self.dword(k) as u16
    }
    pub fn high(&self, k: Slot) -> u8 {
        (self.wide(k) >> 8) as u8
    }
    pub fn low(&self, k: Slot) -> u8 {
        self.wide(k) as u8
    }
    pub fn set_wide(&mut self, k: Slot, v: u16) {
        self.set_dword(k, (self.dword(k) & !0xffff) | v as u32)
    }
    pub fn set_high(&mut self, k: Slot, v: u8) {
        self.set_wide(k, (self.wide(k) & 255) | u16::from(v) << 8)
    }
    pub fn set_low(&mut self, k: Slot, v: u8) {
        self.set_wide(k, (self.wide(k) & 0xff00) | u16::from(v))
    }
    pub fn instruction(&self) -> u16 {
        self.ip as u16
    }
    pub fn set_instruction(&mut self, v: u16) {
        self.ip = v as u32
    }
    pub fn selector(&self, k: Segment) -> u16 {
        (match k {
            Segment::Code => self.cs,
            Segment::Data => self.vds,
            Segment::Extra => self.ves,
            Segment::Stack => self.ss,
        }) as u16
    }
    pub fn set_selector(&mut self, k: Segment, v: u16) {
        *match k {
            Segment::Code => &mut self.cs,
            Segment::Data => &mut self.vds,
            Segment::Extra => &mut self.ves,
            Segment::Stack => &mut self.ss,
        } = v as u32
    }
}
