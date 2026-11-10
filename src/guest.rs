//! Guest 8086 machine.
//!
//! The register file is [`state::CpuState`]. Addresses are linear guest
//! offsets, never host pointers. `INT` returns to the owner after the
//! instruction pointer has moved past the instruction.

pub mod alu;
pub mod memory;
pub mod run;
pub mod state;
pub mod step;
pub mod volume;
