//! `$PARSE_FILE_DESCRIPTOR` from `DOS/PARSE.ASM`.
//!
//! The body is `MakeFcb`. AL comes back as 0, 1, or 0xFF, and SI is the
//! first character the scan did not consume.

pub fn parse(flags: u8, text: *const u8, fcb: *mut u8) -> (u8, *const u8) {
    super::fcb::make_fcb(flags, text, fcb)
}
