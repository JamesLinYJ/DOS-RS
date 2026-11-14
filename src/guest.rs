//! Guest 8086 machine.
//!
//! The register file is [`state::CpuState`]. Addresses are linear guest
//! offsets, never host pointers. `INT` returns to the owner after the
//! instruction pointer has moved past the instruction.

#![forbid(unsafe_code)]

pub mod alu;
mod arena;
pub mod command;
pub mod disk;
pub mod memory;
pub mod mz;
mod process;
pub mod run;
pub mod state;
pub mod step;

#[cfg(feature = "pc386")]
mod pc386;

#[cfg(feature = "pc386")]
mod vga;

#[cfg(feature = "pc386")]
mod at;
