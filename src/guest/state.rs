//! Owned guest register file.
//!
//! Eight words sit in instruction-encoding order. A byte index below four is
//! the low half of that word. A byte index of four through seven is the high
//! half of word zero through three.

const RESERVED_FLAG: u16 = 0x0002;
const ARITH_FLAGS: u16 = 0x08D5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    Accumulator,
    Count,
    Data,
    Base,
    Stack,
    Frame,
    Source,
    Dest,
}

impl Slot {
    pub fn from_index(index: u8) -> Self {
        match index & 7 {
            0 => Self::Accumulator,
            1 => Self::Count,
            2 => Self::Data,
            3 => Self::Base,
            4 => Self::Stack,
            5 => Self::Frame,
            6 => Self::Source,
            _ => Self::Dest,
        }
    }

    pub(super) fn index(self) -> usize {
        match self {
            Self::Accumulator => 0,
            Self::Count => 1,
            Self::Data => 2,
            Self::Base => 3,
            Self::Stack => 4,
            Self::Frame => 5,
            Self::Source => 6,
            Self::Dest => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Segment {
    Extra,
    Code,
    Stack,
    Data,
    #[cfg(feature = "pc386")]
    Fs,
    #[cfg(feature = "pc386")]
    Gs,
}

impl Segment {
    pub fn from_index(index: u8) -> Option<Self> {
        match index & 7 {
            0 => Some(Self::Extra),
            1 => Some(Self::Code),
            2 => Some(Self::Stack),
            3 => Some(Self::Data),
            #[cfg(feature = "pc386")]
            4 => Some(Self::Fs),
            #[cfg(feature = "pc386")]
            5 => Some(Self::Gs),
            _ => None,
        }
    }

    pub(super) fn index(self) -> usize {
        match self {
            Self::Extra => 0,
            Self::Code => 1,
            Self::Stack => 2,
            Self::Data => 3,
            #[cfg(feature = "pc386")]
            Self::Fs => 4,
            #[cfg(feature = "pc386")]
            Self::Gs => 5,
        }
    }
}

#[derive(Debug)]
pub struct CpuState {
    #[cfg(feature = "pc386")]
    pub(super) machine: super::pc386::Machine,
    words: [u32; 8],
    selectors: [u16; 6],
    pub(super) address32: bool,
    instruction: u32,
    flags: u16,
    /// Offset from the most recent memory operand. A register-form load of an
    /// effective address copies this, matching 8086 silicon.
    recent_offset: u16,
}

impl CpuState {
    pub fn at_com(segment: u16) -> Self {
        let mut state = Self {
            #[cfg(feature = "pc386")]
            machine: super::pc386::Machine::new(),
            words: [0; 8],
            selectors: [0; 6],
            address32: false,
            instruction: 0x0100,
            flags: 0x0202,
            recent_offset: 0,
        };
        state.set_selector(Segment::Extra, segment);
        state.set_selector(Segment::Code, segment);
        state.set_selector(Segment::Stack, segment);
        state.set_selector(Segment::Data, segment);
        state.set_wide(Slot::Stack, 0x0400);
        state
    }

    pub fn wide(&self, slot: Slot) -> u16 {
        self.words[slot.index()] as u16
    }

    pub fn set_wide(&mut self, slot: Slot, value: u16) {
        let word = &mut self.words[slot.index()];
        *word = (*word & 0xffff0000) | u32::from(value);
    }

    pub fn dword(&self, slot: Slot) -> u32 {
        self.words[slot.index()]
    }
    pub fn set_dword(&mut self, slot: Slot, value: u32) {
        self.words[slot.index()] = value;
    }
    pub fn low(&self, slot: Slot) -> u8 {
        self.wide(slot) as u8
    }

    pub fn set_low(&mut self, slot: Slot, value: u8) {
        let mixed = (self.wide(slot) & 0xFF00) | u16::from(value);
        self.set_wide(slot, mixed);
    }

    pub fn high(&self, slot: Slot) -> u8 {
        (self.wide(slot) >> 8) as u8
    }

    pub fn set_high(&mut self, slot: Slot, value: u8) {
        let mixed = (self.wide(slot) & 0x00FF) | (u16::from(value) << 8);
        self.set_wide(slot, mixed);
    }

    pub fn selector(&self, which: Segment) -> u16 {
        self.selectors[which.index()]
    }

    pub fn set_selector(&mut self, which: Segment, value: u16) {
        self.selectors[which.index()] = value;
        #[cfg(feature = "pc386")]
        if !self.machine.protected() || self.machine.vm86 {
            self.machine.real_segment(which, value);
        }
    }

    pub(super) fn linear(
        &self,
        ram: &mut super::memory::GuestRam,
        seg: Segment,
        offset: impl Into<u32>,
        write: bool,
    ) -> Option<usize> {
        #[cfg(feature = "pc386")]
        {
            self.machine.linear(ram, seg, offset.into(), write)
        }
        #[cfg(not(feature = "pc386"))]
        {
            let _ = write;
            Some(ram.address(self.selector(seg), offset.into() as u16))
        }
    }
    pub(super) fn load_byte(
        &self,
        ram: &mut super::memory::GuestRam,
        seg: Segment,
        offset: impl Into<u32>,
    ) -> Option<u8> {
        let address = self.linear(ram, seg, offset, false)?;
        Some(ram.bus_byte(address))
    }
    pub(super) fn store_byte(
        &self,
        ram: &mut super::memory::GuestRam,
        seg: Segment,
        offset: impl Into<u32>,
        value: u8,
    ) -> bool {
        if let Some(at) = self.linear(ram, seg, offset, true) {
            ram.set_bus_byte(at, value);
            true
        } else {
            false
        }
    }
    pub fn flags(&self) -> u16 {
        self.flags
    }

    pub fn set_flags(&mut self, value: u16) {
        self.flags = value | RESERVED_FLAG;
    }

    pub fn instruction(&self) -> u16 {
        self.instruction as u16
    }

    pub fn set_instruction(&mut self, value: u16) {
        self.instruction = u32::from(value);
    }

    pub fn eflags(&self) -> u32 {
        let flags = u32::from(self.flags());
        #[cfg(feature = "pc386")]
        {
            return flags | u32::from(self.machine.vm86) << 17;
        }
        #[cfg(not(feature = "pc386"))]
        flags
    }
    pub fn stack_pointer(&self) -> u32 {
        #[cfg(feature = "pc386")]
        if self.machine.stack32() {
            return self.dword(Slot::Stack);
        }
        u32::from(self.wide(Slot::Stack))
    }
    pub fn set_stack_pointer(&mut self, v: u32) {
        #[cfg(feature = "pc386")]
        if self.machine.stack32() {
            self.set_dword(Slot::Stack, v);
            return;
        }
        self.set_wide(Slot::Stack, v as u16)
    }
    pub fn eip(&self) -> u32 {
        self.instruction
    }
    pub fn set_eip(&mut self, v: u32) {
        self.instruction = v;
    }
    pub fn advance_ip(&mut self, disp: i32) {
        let mut next = self.eip().wrapping_add_signed(disp);
        #[cfg(feature = "pc386")]
        let wide = self.machine.code32();
        #[cfg(not(feature = "pc386"))]
        let wide = false;
        if !wide {
            next &= 65535
        }
        self.set_eip(next);
    }
    pub fn recent_offset(&self) -> u16 {
        self.recent_offset
    }

    pub fn set_recent_offset(&mut self, value: u16) {
        self.recent_offset = value;
    }

    pub fn set_carry(&mut self, carry: bool) {
        if carry {
            self.flags |= 1;
        } else {
            self.flags &= !1;
        }
    }

    pub fn set_zero(&mut self, zero: bool) {
        const ZERO: u16 = 0x0040;
        if zero {
            self.flags |= ZERO;
        } else {
            self.flags &= !ZERO;
        }
    }

    pub fn direction_down(&self) -> bool {
        self.flags & 0x0400 != 0
    }

    pub fn set_interrupt(&mut self, enabled: bool) {
        const INTERRUPT: u16 = 0x0200;
        if enabled {
            self.flags |= INTERRUPT;
        } else {
            self.flags &= !INTERRUPT;
        }
    }

    pub fn set_direction(&mut self, down: bool) {
        const DIRECTION: u16 = 0x0400;
        if down {
            self.flags |= DIRECTION;
        } else {
            self.flags &= !DIRECTION;
        }
    }

    pub fn apply_result_flags(&mut self, computed: u16) {
        self.flags = (self.flags & !ARITH_FLAGS) | (computed & ARITH_FLAGS) | RESERVED_FLAG;
    }
}
