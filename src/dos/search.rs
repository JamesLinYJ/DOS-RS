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
    pub attr: u8,
    pub time: u16,
    pub date: u16,
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
    attr: 0,
    time: 0,
    date: 0,
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
                ROOT[index].attr = entry[11];
                ROOT[index].time = u16::from_le_bytes([entry[22], entry[23]]);
                ROOT[index].date = u16::from_le_bytes([entry[24], entry[25]]);
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

pub(crate) fn walk(mut visit: impl FnMut(usize, Ent)) {
    ensure();
    let mut index = 0usize;
    while index < SLOTS {
        let ent = slot(index);
        if ent.live {
            visit(index, ent);
        }
        index += 1;
    }
}

pub(crate) fn for_live(mut visit: impl FnMut([u8; 11])) {
    ensure();
    unsafe {
        let mut index = 0usize;
        while index < SLOTS {
            if ROOT[index].live {
                visit(ROOT[index].name);
            }
            index += 1;
        }
    }
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

pub fn dta() -> *mut u8 {
    unsafe { DTA }
}

pub fn find_first(name: *const u8, attr: u8) -> u16 {
    ensure();
    let raw = read_asciiz(name);
    let text = &raw[..asciiz_len(&raw)];
    unsafe {
        SATTR = attr;
        STAR = text == b"*.*" || text == b"*";
        if !STAR {
            match pack(text) {
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
        #[allow(clippy::deref_addrof)]
        let pat = unsafe { *(&raw const PAT) };
        let matched = unsafe { STAR } || ent.name.eq_ignore_ascii_case(&pat);
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
    let file_attr = if ent.attr != 0 {
        ent.attr
    } else if ent.dir {
        0x10
    } else {
        0x20
    };
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
        *dta.add(0x15) = if ent.attr != 0 {
            ent.attr
        } else if ent.dir {
            0x10
        } else {
            0x20
        };
        let time = ent.time.to_le_bytes();
        let date = ent.date.to_le_bytes();
        *dta.add(0x16) = time[0];
        *dta.add(0x17) = time[1];
        *dta.add(0x18) = date[0];
        *dta.add(0x19) = date[1];
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

pub(crate) fn lookup_asciiz(name: *const u8) -> Option<usize> {
    ensure();
    let raw = read_asciiz(name);
    let text = &raw[..asciiz_len(&raw)];
    let text = text.strip_prefix(b"A:\\").unwrap_or(text);
    let text = text.strip_prefix(b"A:").unwrap_or(text);
    let packed = pack(text)?;
    find_packed(&packed)
}

pub(crate) fn set_attr(index: usize, attr: u8) {
    let mut ent = slot(index);
    ent.attr = attr;
    set_slot(index, ent);
}

pub(crate) fn set_stamp(index: usize, time: u16, date: u16) {
    let mut ent = slot(index);
    ent.time = time;
    ent.date = date;
    set_slot(index, ent);
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
}

static mut FCB_NEXT: usize = 0;
static mut FCB_PAT: [u8; 11] = [b' '; 11];
static mut FCB_ATTR: u8 = 0;

pub(crate) fn name_match(pat: &[u8; 11], name: &[u8; 11]) -> bool {
    let mut i = 0usize;
    while i < 11 {
        if pat[i] != b'?' && pat[i] != name[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn fcb_attr_ok(file_attr: u8, search_attr: u8) -> bool {
    if search_attr & 0x08 != 0 {
        return file_attr & 0x08 != 0;
    }
    const SPECIAL: u8 = 0x02 | 0x04 | 0x08 | 0x10;
    file_attr & SPECIAL & !search_attr == 0
}

fn fcb_find(start: usize) -> Option<usize> {
    let pat = unsafe { FCB_PAT };
    let attr = unsafe { FCB_ATTR };
    let mut index = start;
    while index < SLOTS {
        let ent = slot(index);
        if ent.live && fcb_attr_ok(ent.attr, attr) && name_match(&pat, &ent.name) {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn write_fcb_dir(ent: &Ent, dta: *mut u8) {
    if dta.is_null() {
        return;
    }
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(&ent.name);
    raw[11] = ent.attr;
    raw[22] = ent.time as u8;
    raw[23] = (ent.time >> 8) as u8;
    raw[24] = ent.date as u8;
    raw[25] = (ent.date >> 8) as u8;
    raw[26] = ent.cluster as u8;
    raw[27] = (ent.cluster >> 8) as u8;
    let size = if ent.owned {
        u32::from(ent.data_len)
    } else {
        u32::from(ent.size)
    };
    raw[28..32].copy_from_slice(&size.to_le_bytes());
    let mut i = 0usize;
    while i < 32 {
        unsafe { *dta.add(i) = raw[i] }
        i += 1;
    }
}

/// AH=11h. A volume-attribute search misses every file on this disk.
pub fn fcb_search_first(fcb: *const u8) -> u8 {
    ensure();
    let (_, attr) = crate::dos::fcb::split(fcb);
    unsafe {
        FCB_ATTR = attr;
        FCB_PAT = crate::dos::fcb::packed_name(fcb);
        FCB_NEXT = 0;
    }
    fcb_search_next()
}

/// AH=12h.
pub fn fcb_search_next() -> u8 {
    ensure();
    let start = unsafe { FCB_NEXT };
    match fcb_find(start) {
        Some(index) => {
            unsafe { FCB_NEXT = index + 1 }
            write_fcb_dir(&slot(index), dta());
            0
        }
        None => 0xff,
    }
}
