//! MS-DOS kernel pieces the BIOS init path calls through INT 21h.
//!
//! The checklist already maps `DOS/CPMIO.ASM` and `DOS/CPMIO2.ASM` here.
//! This slice is only the console output those files use for a `$` string
//! and a single character. No new register type is added. The frame stays
//! in `int21`.

pub mod abort;
pub mod alloc;
pub mod buf;
pub mod close;
pub mod crit;
pub mod ctrlc;
pub mod cpmio;
pub mod cpmio2;
pub mod disp;
pub mod exec;
pub mod fcb;
pub mod fcbio;
pub mod fcbio2;
pub mod fat;
pub mod file;
pub mod getset;
pub mod dev;
pub mod dinfo;
pub mod disk;
pub mod handle;
pub mod ioctl;
pub mod misc;
pub mod ms_code;
pub mod parse;
pub mod path;
pub mod rom;
pub mod search;
