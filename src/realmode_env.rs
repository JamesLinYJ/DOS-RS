//! DOS environment blocks and their trailing executable path. The transient
//! shell's environment lives outside the program arena; EXEC children get an
//! owned MCB containing their own copy and fully qualified program name.
use dosrs_realmode::{
    self as pc, byte, store,
    system::{sw, w},
};
// DOS keeps a current directory on each drive, even while another is active.
const ROOT_ENV: u16 = 0x30b0;
fn path_at(drive: u16) -> u16 {
    let slot = if drive < 3 { drive } else { 3 };
    pc::dos_tables::C_PATH - 2 * 88 + slot * 88
}
pub fn path_len(drive: u16) -> u16 {
    let mut n = 0;
    let base = path_at(drive);
    while n < 63 && byte(pc::dos_tables::LIST_SEG, base + n) != 0 {
        n += 1
    }
    n
}
pub fn cwd(drive: u16, seg: u16, at: u16) {
    let n = path_len(drive);
    pc::copy_bytes(pc::dos_tables::LIST_SEG, path_at(drive), seg, at, n);
    store(seg, at + n, 0);
}
/// Canonicalize relative components against the drive's published CDS. The
/// caller updates the CDS only after the volume has accepted the full path.
pub fn canonical(
    drive: u16,
    seg: u16,
    mut at: u16,
    end: u16,
    dst: u16,
    out: u16,
    limit: u16,
) -> Option<u16> {
    store(dst, out, 92);
    let mut n = 1;
    if at >= end || byte(seg, at) != 92 {
        cwd(drive, dst, out + 1);
        n += path_len(drive);
    }
    while at < end {
        if byte(seg, at) == 92 {
            at += 1;
            continue;
        }
        let begin = at;
        while at < end && byte(seg, at) != 92 {
            at += 1
        }
        let len = at - begin;
        if len == 1 && byte(seg, begin) == 46 {
            continue;
        }
        if len == 2 && w(seg, begin) == 0x2e2e {
            while n > 1 && byte(dst, out + n - 1) != 92 {
                n -= 1
            }
            if n > 1 {
                n -= 1
            }
            continue;
        }
        if n + len + u16::from(n > 1) > limit {
            return None;
        }
        if n > 1 {
            store(dst, out + n, 92);
            n += 1
        }
        let mut i = begin;
        while i < at {
            let c = byte(seg, i);
            store(
                dst,
                out + n,
                if (97..=122).contains(&c) { c - 32 } else { c },
            );
            i += 1;
            n += 1;
        }
    }
    store(dst, out + n, 0);
    Some(n)
}
pub fn remember(drive: u16, seg: u16, abs: u16, len: u16) {
    pc::copy_bytes(seg, abs + 1, pc::dos_tables::LIST_SEG, path_at(drive), len);
}
pub fn root(psp: u16) {
    sw(psp, 0x2c, ROOT_ENV);
    let drive = if pc::boot_unit() >= 128 {
        2
    } else {
        pc::boot_unit()
    };
    let prefix = b"COMSPEC=";
    let mut i = 0;
    while i < prefix.len() {
        store(ROOT_ENV, i as u16, u16::from(prefix[i]));
        i += 1;
    }
    store(ROOT_ENV, 8, 65 + drive);
    let file = b":\\COMMAND.COM";
    i = 0;
    while i < file.len() {
        store(ROOT_ENV, 9 + i as u16, u16::from(file[i]));
        i += 1;
    }
    let end = 9 + file.len() as u16;
    sw(ROOT_ENV, end, 0);
    sw(ROOT_ENV, end + 2, 1);
    pc::copy_bytes(ROOT_ENV, 8, ROOT_ENV, end + 4, end - 8 + 1);
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
    let index = (drive & 0xdf).wrapping_sub(65);
    let prefix = if absolute { 0 } else { path_len(index) };
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
        cwd(index, dest, at);
        at += prefix;
        store(dest, at, 92);
        at += 1;
    }
    pc::copy_bytes(pathseg, path, dest, at, count + 1);
    Some(dest)
}
