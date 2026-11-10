//! `$FIND_FIRST` and `$FIND_NEXT` from `DOS/SEARCH.ASM`.
//!
//! The root is the boot image, then the directory calls mutate it. The DTA
//! layout is the DOS one: attribute at 0x15 and the 8.3 name at 0x1E.

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");
const SLOTS: usize = 16;
const NO_MORE: u16 = 18;

#[derive(Clone, Copy)]
pub(crate) struct Ent {
    pub name: [u8; 11],
    pub live: bool,
    pub dir: bool,
    pub owned: bool,
    pub data: [u8; 32],
    pub data_len: u8,
    pub cluster: u16,
    pub size: u16,
}

static mut ROOT: [Ent; SLOTS] = [Ent {
    name: [0; 11],
    live: false,
    dir: false,
    owned: false,
    data: [0; 32],
    data_len: 0,
    cluster: 0,
    size: 0,
}; SLOTS];
static mut READY: bool = false;
static mut CWD: [u8; 32] = *b"A:\\\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
static mut CWD_LEN: usize = 3;
static mut DTA: *mut u8 = core::ptr::null_mut();
static mut NEXT: usize = 0;
static mut SATTR: u8 = 0;
static mut STAR: bool = false;
static mut PAT: [u8; 11] = [b' '; 11];

pub(crate) fn ensure() {
    unsafe {
        if READY {
            return;
        }
        let root = 3 * 512;
        let mut index = 0usize;
        while index < SLOTS {
            let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
            if entry[0] == 0 {
                break;
            }
            if entry[0] != 0xe5 && entry[11] & 0x08 == 0 {
                let mut name = [0u8; 11];
                name.copy_from_slice(&entry[..11]);
                ROOT[index].name = name;
                ROOT[index].live = true;
                ROOT[index].cluster = u16::from_le_bytes([entry[26], entry[27]]);
                ROOT[index].size = u16::from_le_bytes([entry[28], entry[29]]);
            }
            index += 1;
        }
        READY = true;
    }
}

pub fn cwd_text() -> &'static [u8] {
    ensure();
    unsafe { &CWD[..CWD_LEN] }
}

pub(crate) fn set_cwd(text: &[u8]) {
    let n = text.len().min(32);
    unsafe {
        CWD[..n].copy_from_slice(&text[..n]);
        CWD_LEN = n;
    }
}

pub(crate) fn cwd_is_root() -> bool {
    ensure();
    unsafe { CWD_LEN == 3 }
}

pub(crate) fn slot(index: usize) -> Ent {
    unsafe { ROOT[index] }
}

pub(crate) fn set_slot(index: usize, ent: Ent) {
    unsafe { ROOT[index] = ent }
}

pub(crate) fn find_packed(name: &[u8; 11]) -> Option<usize> {
    ensure();
    let mut index = 0usize;
    while index < SLOTS {
        let ent = slot(index);
        if ent.live && ent.name.eq_ignore_ascii_case(name) {
            return Some(index);
        }
        index += 1;
    }
    None
}

pub(crate) fn alloc_slot() -> Option<usize> {
    ensure();
    let mut index = 0usize;
    while index < SLOTS {
        if !slot(index).live {
            return Some(index);
        }
        index += 1;
    }
    None
}

pub fn set_dta(ptr: *mut u8) {
    unsafe { DTA = ptr }
}

pub fn find_first(name: *const u8, attr: u8) -> u16 {
    ensure();
    let raw = read_asciiz(name);
    let text = &raw[..asciiz_len(&raw)];
    unsafe {
        SATTR = attr;
        STAR = text == b"*.*" || text == b"*";
        if !STAR {
            match pack(&text) {
                Some(pat) => PAT = pat,
                None => return NO_MORE,
            }
        }
        NEXT = 0;
    }
    find_next()
}

pub fn find_next() -> u16 {
    ensure();
    loop {
        let index = unsafe { NEXT };
        if index >= SLOTS {
            return NO_MORE;
        }
        unsafe { NEXT += 1 }
        let ent = slot(index);
        if !visible(ent) {
            continue;
        }
        let matched = unsafe { STAR || ent.name.eq_ignore_ascii_case(&PAT) };
        if !matched {
            continue;
        }
        write_dta(ent, index);
        return 0;
    }
}

fn visible(ent: Ent) -> bool {
    if !ent.live {
        return false;
    }
    let file_attr = if ent.dir { 0x10u8 } else { 0x20 };
    let sattr = unsafe { SATTR };
    file_attr & 0x1e & !sattr == 0
}

fn write_dta(ent: Ent, index: usize) {
    let dta = unsafe { DTA };
    if dta.is_null() {
        return;
    }
    unsafe {
        *dta.add(1) = (index as u8).wrapping_add(1);
        *dta.add(0x15) = if ent.dir { 0x10 } else { 0x20 };
        let mut at = 0x1eusize;
        let mut n = 0usize;
        while n < 8 && ent.name[n] != b' ' {
            *dta.add(at) = ent.name[n];
            at += 1;
            n += 1;
        }
        if !ent.dir {
            let mut e = 0usize;
            let mut any = false;
            while e < 3 && ent.name[8 + e] != b' ' {
                any = true;
                e += 1;
            }
            if any {
                *dta.add(at) = b'.';
                at += 1;
                e = 0;
                while e < 3 && ent.name[8 + e] != b' ' {
                    *dta.add(at) = ent.name[8 + e];
                    at += 1;
                    e += 1;
                }
            }
        }
        *dta.add(at) = 0;
    }
}

pub(crate) fn pack(name: &[u8]) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let mut dot = name.len();
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    if dot == 0 || dot > 8 {
        return None;
    }
    let mut n = 0usize;
    while n < dot {
        out[n] = up(name[n]);
        n += 1;
    }
    if dot < name.len() {
        let mut e = 0usize;
        while e < 3 && dot + 1 + e < name.len() {
            out[8 + e] = up(name[dot + 1 + e]);
            e += 1;
        }
    }
    Some(out)
}

pub(crate) fn read_asciiz(name: *const u8) -> [u8; 32] {
    let mut raw = [0u8; 32];
    let mut len = 0usize;
    unsafe {
        while len < 31 {
            let byte = *name.add(len);
            if byte == 0 {
                break;
            }
            raw[len] = byte;
            len += 1;
        }
    }
    raw
}

pub(crate) fn asciiz_len(raw: &[u8]) -> usize {
    let mut len = 0usize;
    while len < raw.len() && raw[len] != 0 {
        len += 1;
    }
    len
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
}
