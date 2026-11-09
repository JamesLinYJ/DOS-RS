//! MS-DOS kernel pieces the BIOS init path calls through INT 21h.
//!
//! The checklist already maps `DOS/CPMIO.ASM` and `DOS/CPMIO2.ASM` here.
//! This slice is only the console output those files use for a `$` string
//! and a single character. No new register type is added. The frame stays
//! in `int21`.

pub mod cpmio;
pub mod cpmio2;
