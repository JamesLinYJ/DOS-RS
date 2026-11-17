//! DOS environment blocks and their trailing executable path. The transient
//! shell's environment lives outside the program arena; EXEC children get an
//! owned MCB containing their own copy and fully qualified program name.
use dosrs_realmode::{
    self as pc, byte, store,
    system::{sw, w},
};
/// Between the optical transfer buffer and FAT scratch. CD stores the active path without a leading slash.
const CWD: u16 = 0x30a0;
const ROOT_ENV: u16 = 0x30b0;

pub fn path_change(seg: u16, mut at: u16, end: u16) {
    let mut n = byte(CWD, 0);
    if at < end && byte(seg, at) == 92 {
        n = 0;
        at += 1;
    }
    while at < end {
        let start = at;
        while at < end && byte(seg, at) != 92 {
            at += 1
        }
        let len = at - start;
        if len == 2 && w(seg, start) == 0x2e2e {
            while n != 0 && byte(CWD, n) != 92 {
                n -= 1;
            }
            if n != 0 {
                n -= 1;
            }
        } else if len != 0 && !(len == 1 && byte(seg, start) == 46) {
            if n + len + 1 > 63 {
                return;
            }
            if n != 0 {
                n += 1;
                store(CWD, n, 92);
            }
            pc::copy_bytes(seg, start, CWD, n + 1, len);
            n += len;
        }
        if at < end {
            at += 1;
        }
    }
    store(CWD, 0, n);
    store(CWD, n + 1, 0);
    pc::copy_bytes(
        CWD,
        1,
        pc::dos_tables::LIST_SEG,
        pc::dos_tables::C_PATH,
        n + 1,
    );
}
pub fn cwd(seg: u16, at: u16) {
    pc::copy_bytes(CWD, 1, seg, at, byte(CWD, 0) + 1);
}
pub fn root(psp: u16) {
    sw(psp, 0x2c, ROOT_ENV);
    // Root has an empty environment. The two NUL bytes precede the extra string count.
    sw(ROOT_ENV, 0, 0);
    sw(ROOT_ENV, 2, 1);
    let drive = byte(pc::COMMAND, 256);
    sw(ROOT_ENV, 4, (65 + drive) | 0x3a00);
    store(ROOT_ENV, 6, 92);
    let mut out = 7;
    let prefix = byte(CWD, 0);
    if drive == 2 && prefix != 0 {
        pc::copy_bytes(CWD, 1, ROOT_ENV, out, prefix);
        out += prefix;
        store(ROOT_ENV, out, 92);
        out += 1;
    }
    let mut i = 0;
    while i < 11 {
        let c = byte(pc::COMMAND, 128 + i);
        if i == 8 && c != 32 {
            store(ROOT_ENV, out, 46);
            out += 1;
        }
        if c != 32 {
            store(ROOT_ENV, out, c);
            out += 1;
        }
        i += 1;
    }
    store(ROOT_ENV, out, 0);
}
pub fn child(parent: u16, env: u16, pathseg: u16, mut path: u16) -> Option<u16> {
    let src = if env == 0 { w(parent, 0x2c) } else { env };
    let mut len = 0;
    if src != 0 {
        while len < 32766 && w(src, len) != 0 {
            len += 1;
        }
        if len == 32766 {
            return None;
        }
    }
    len += 2;
    let mut drive = byte(pc::COMMAND, 256) + 65;
    if byte(pathseg, path + 1) == 58 {
        drive = byte(pathseg, path);
        path += 2;
    }
    let absolute = byte(pathseg, path) == 92;
    if absolute {
        path += 1;
    }
    let mut count = 0;
    while count < 128 && byte(pathseg, path + count) != 0 {
        count += 1;
    }
    if count == 128 {
        return None;
    }
    let prefix = if absolute { 0 } else { byte(CWD, 0) };
    let bytes = len + 2 + 3 + prefix + 1 + count + 1;
    let mut largest = 0;
    let r = super::arena::request(0x4800, (bytes + 15) >> 4, 0, &mut largest);
    if r > 65535 {
        return None;
    }
    let dest = r as u16;
    if src != 0 {
        pc::copy_bytes(src, 0, dest, 0, len)
    } else {
        sw(dest, 0, 0)
    }
    sw(dest, len, 1);
    sw(dest, len + 2, drive | 0x3a00);
    store(dest, len + 4, 92);
    let mut at = len + 5;
    if prefix != 0 {
        pc::copy_bytes(CWD, 1, dest, at, prefix);
        at += prefix;
        store(dest, at, 92);
        at += 1;
    }
    pc::copy_bytes(pathseg, path, dest, at, count + 1);
    Some(dest)
}
