#![no_std]
// This MZ application links against the normal DOS runtime. The filename,
// argument and success text are observed through DOS services at run time.
#[path = "../../src/dos/fat_cluster.rs"]
mod cluster;
unsafe extern "Rust" {
    fn arg(index: u16, byte: u16) -> u16;
    fn putch(byte: u16) -> u16;
}
#[inline(never)]
fn check(max: u16, n: u16, offset: usize, high: bool) -> bool {
    match cluster::map_cluster(max, n) {
        Some((at, odd)) => at == offset && odd == high,
        None => false,
    }
}
fn text(s: &str) {
    let mut i = 0;
    while i < s.len() {
        unsafe {
            putch(s.as_bytes()[i] as u16);
        }
        i += 1;
    }
}
#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let n = unsafe { arg(1, 0) };
    let passed = n == 65
        && check(4085, n, 97, true)
        && check(4085, n + 1, 99, false)
        && check(4086, n, 130, false)
        && check(65524, 32767, 65534, false)
        && cluster::map_cluster(65524, 32768).is_none()
        && cluster::map_cluster(61, n).is_none()
        && cluster::is_eof(61, 0xff8)
        && !cluster::is_eof(61, 0xff7)
        && cluster::is_eof(4086, 0xfff8)
        && !cluster::is_eof(4086, 0xfff7);
    if passed {
        text("FAT ABI OK\r\n");
        0
    } else {
        text("FAT ABI FAIL\r\n");
        1
    }
}
