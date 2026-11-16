#![no_std]
#![no_main]
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    dos_tools::editor::run()
}
