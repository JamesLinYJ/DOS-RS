//! Decode an independent LZ4 block between two real-mode segments.
use dosrs_realmode::{byte, copy_bytes, system::w};
pub const STAGE: u16 = dosrs_realmode::PROGRAM;
fn length(src: u16, input: &mut u16, end: u16, part: u16) -> Option<u16> {
    let mut len = part;
    if part == 15 {
        loop {
            if *input >= end {
                return None;
            }
            let v = byte(src, *input);
            *input += 1;
            if len > 0xffff - v {
                return None;
            }
            len += v;
            if v != 255 {
                break;
            }
        }
    }
    Some(len)
}
pub fn unpack(src: u16, end: u16, size: u16, dest: u16) -> Option<()> {
    let mut i = 0;
    let mut out = 0u16;
    while i < end {
        let token = byte(src, i);
        i += 1;
        let lit = length(src, &mut i, end, token >> 4)?;
        if lit > end - i || lit > size - out {
            return None;
        }
        copy_bytes(src, i, dest, out, lit);
        i += lit;
        out += lit;
        if i == end {
            return if out == size { Some(()) } else { None };
        }
        if end - i < 2 {
            return None;
        }
        let distance = w(src, i);
        i += 2;
        let n = length(src, &mut i, end, token & 15)?;
        if n > 0xfffb {
            return None;
        }
        let count = n + 4;
        if distance == 0 || distance > out || count > size - out {
            return None;
        }
        copy_bytes(dest, out - distance, dest, out, count);
        out += count;
    }
    None
}
