//! Per-session x86 guest execution for DOS programs and 16-bit protected-mode clients.
//!
//! Instruction fetch, operands and virtual devices use offsets on the guest memory bus.
//! The scheduler handles DOS services and physical I/O after a slice yields.

#![forbid(unsafe_code)]

pub mod alu;
mod arena;
pub mod command;
pub mod disk;
pub mod memory;
pub mod mz;
mod process;
mod resident;
pub mod run;
pub mod state;
pub mod step;

#[cfg(feature = "pc386")]
mod pc386;

#[cfg(feature = "pc386")]
mod vga;

#[cfg(feature = "pc386")]
mod at;
