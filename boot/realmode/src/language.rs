//! Language selected by CONFIG.SYS, in the same order as the installer.
use crate::{COMMAND, byte, store};
pub const ENGLISH: u16 = 1;
const OFFSET: u16 = 271;
pub fn tag(id: u16) -> &'static str {
    match id {
        0 => "de",
        2 => "es",
        3 => "fr",
        4 => "ja",
        5 => "ko",
        6 => "zh-Hans",
        7 => "zh-Hant",
        _ => "en",
    }
}
pub fn set(id: u16) {
    store(COMMAND, OFFSET, id);
}
pub fn selected() -> u16 {
    let id = byte(COMMAND, OFFSET);
    if id < 8 { id } else { ENGLISH }
}
