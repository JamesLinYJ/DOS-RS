//! Resident IO.SYS optical service. The buffer descriptor is shared for one synchronous call.
use super::{
    byte, control,
    system::{dw, sd, sw},
};
const REQUEST: u16 = 0x35e0;
pub fn probe() -> bool {
    byte(0x3800, 270) != 0 && control(16, 0, 0, 0) != 0
}
pub fn present() -> bool {
    control(17, 0, 0, 0) != 0
}
pub fn letter() -> u16 {
    let n = byte(0x40, 0x75);
    2 + if n == 0 { 1 } else { n }
}
pub fn list() -> bool {
    control(18, 0, 0, 0) != 0
}
pub fn find(seg: u16, at: u16, end: u16, dir: bool) -> bool {
    control(if dir { 20 } else { 19 }, seg, at, end) != 0
}
pub fn display() -> bool {
    control(21, 0, 0, 0) != 0
}
pub fn extent() -> u32 {
    dw(0x35c0, 20)
}
pub fn length() -> u32 {
    dw(0x35c0, 24)
}
pub fn fetch(lba: u32, size: u32, pos: u32, count: u16, seg: u16, at: u16) -> u32 {
    sd(REQUEST, 0, lba);
    sd(REQUEST, 4, size);
    sd(REQUEST, 8, pos);
    sw(REQUEST, 12, count);
    sw(REQUEST, 14, seg);
    sw(REQUEST, 16, at);
    if control(22, 0, 0, 0) != 0 {
        dw(REQUEST, 20)
    } else {
        u32::MAX
    }
}
