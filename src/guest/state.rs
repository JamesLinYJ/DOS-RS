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

    fn index(self) -> usize {
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
}

impl Segment {
    pub fn from_index(index: u8) -> Option<Self> {
        match index & 7 {
            0 => Some(Self::Extra),
            1 => Some(Self::Code),
            2 => Some(Self::Stack),
            3 => Some(Self::Data),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Extra => 0,
            Self::Code => 1,
            Self::Stack => 2,
            Self::Data => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CpuState {
    words: [u16; 8],
    selectors: [u16; 4],
    instruction: u16,
    flags: u16,
}

impl CpuState {
    pub fn at_com(segment: u16) -> Self {
        let mut state = Self {
            words: [0; 8],
            selectors: [0; 4],
            instruction: 0x0100,
            flags: 0x0202,
        };
        state.set_selector(Segment::Extra, segment);
        state.set_selector(Segment::Code, segment);
        state.set_selector(Segment::Stack, segment);
        state.set_selector(Segment::Data, segment);
        state.set_wide(Slot::Stack, 0x0400);
        state
    }

    pub fn wide(&self, slot: Slot) -> u16 {
        self.words[slot.index()]
    }

    pub fn set_wide(&mut self, slot: Slot, value: u16) {
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
    }

    pub fn flags(&self) -> u16 {
        self.flags
    }

    pub fn instruction(&self) -> u16 {
        self.instruction
    }

    pub fn set_instruction(&mut self, value: u16) {
        self.instruction = value;
    }

    pub fn set_carry(&mut self, carry: bool) {
        if carry {
            self.flags |= 1;
        } else {
            self.flags &= !1;
        }
    }

    pub fn apply_result_flags(&mut self, computed: u16) {
        self.flags = (self.flags & !ARITH_FLAGS) | (computed & ARITH_FLAGS) | RESERVED_FLAG;
    }
}
