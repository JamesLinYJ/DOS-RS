//! Report the actual PSP, granted size, free UMB and link state of an EXEC child.
#![no_std]
#![no_main]
use dos_tools as _;
use dosrs_realmode::system::w;
#[path = "support/command_probe.rs"]
mod probe;
use probe::{call, hex, write};

#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let psp = call(0x6200, 0, 0, 0)[1];
    let prior = call(0x5800, 0, 0, 0)[0];
    let link = call(0x5802, 0, 0, 0)[0] & 255;
    call(0x5801, 0x40, 0, 0);
    let free = call(0x4800, u16::MAX, 0, 0)[1];
    call(0x5801, prior, 0, 0);
    let mut line = *b"PSP=0000 SIZE=0000 FREE=0000 LINK=0000\r\n";
    hex(&mut line, 4, psp);
    hex(&mut line, 14, w(psp, 2) - psp);
    hex(&mut line, 24, free);
    hex(&mut line, 34, link);
    write(1, &line);
    0
}
