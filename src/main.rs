//! Boot entry. One `kmain` per target binary.
//!
//! Starts the kernel services, then enters the interactive DOS scheduler or the selected test profile.

#![no_std]
#![no_main]

mod version;

#[cfg(all(feature = "pc386", not(target_pointer_width = "16")))]
extern crate alloc;
#[cfg(not(target_pointer_width = "16"))]
mod arch;
#[cfg(not(target_pointer_width = "16"))]
mod bios;
#[cfg(not(target_pointer_width = "16"))]
mod boot;
#[cfg(not(target_pointer_width = "16"))]
mod cmd;
#[cfg(not(target_pointer_width = "16"))]
mod dos;
#[cfg(not(target_pointer_width = "16"))]
mod drivers;
#[cfg(not(target_pointer_width = "16"))]
mod guest;
#[cfg(all(feature = "pc386", not(target_pointer_width = "16")))]
mod heap;
#[cfg(not(target_pointer_width = "16"))]
mod int21;
#[cfg(not(target_pointer_width = "16"))]
mod kernel_cell;
#[cfg(not(target_pointer_width = "16"))]
mod platform;
#[cfg(not(target_pointer_width = "16"))]
mod smoke;

#[cfg(not(target_pointer_width = "16"))]
include!("wide.rs");

#[cfg(target_pointer_width = "16")]
mod realmode;
