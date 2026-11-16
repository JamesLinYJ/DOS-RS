#![no_std]
#![no_main]
use dos_tools::{argument, install::number, text};
use dosrs_realmode::{cache, hard_unit, hd, raw_hd};
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    if argument(1, "/OFF") {
        cache::enable(false)
    } else if argument(1, "/ON") {
        cache::enable(true)
    } else if argument(1, "/RESET") {
        cache::reset()
    } else if argument(1, "/BENCH") {
        let mut n = 0;
        while n < 320 {
            if !raw_hd(hard_unit(), (n & 31) as u32, 0x3400, false) {
                text("Disk read error\r\n");
                return 1;
            }
            n += 1;
        }
        let active = cache::slots() != 0;
        cache::init();
        if cache::slots() == 0 {
            text("Insufficient cache memory\r\n");
            return 1;
        }
        n = 0;
        while n < 320 {
            if !hd((n & 31) as u32, 0x3400, false) {
                return 1;
            }
            n += 1;
        }
        let hits = cache::hits();
        let reads = cache::reads();
        if !active {
            cache::enable(false)
        }
        text("Uncached BIOS reads 320; cached BIOS reads ");
        number(reads);
        text("; hits ");
        number(hits);
        text("\r\n");
        return 0;
    } else if !argument(1, "") && !argument(1, "/STATUS") {
        text("SMARTDRV /STATUS /ON /OFF /RESET /BENCH\r\n");
        return 1;
    }
    let hits = cache::hits();
    let reads = cache::reads();
    text("SMARTDRV  write-through  sectors ");
    number(cache::slots() as u32);
    text("  hits ");
    number(hits);
    text("  BIOS reads ");
    number(reads);
    text("\r\n");
    0
}
