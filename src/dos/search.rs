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

use crate::kernel_cell::KernelCell;
use core::sync::atomic::{AtomicPtr, Ordering};

const EMPTY: Ent = Ent {
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
};

const fn image_root() -> [Ent; SLOTS] {
    let mut root = [EMPTY; SLOTS];
    let mut i = 0;
    while i < SLOTS {
        let at = 3 * 512 + i * 32;
        if IMAGE[at] == 0 {
            break;
        }
        if IMAGE[at] != 0xe5 && IMAGE[at + 11] & 8 == 0 {
            let mut j = 0;
            while j < 11 {
                root[i].name[j] = IMAGE[at + j];
                j += 1;
            }
            root[i].live = true;
            root[i].cluster = u16::from_le_bytes([IMAGE[at + 26], IMAGE[at + 27]]);
            root[i].size = u16::from_le_bytes([IMAGE[at + 28], IMAGE[at + 29]]);
            root[i].attr = IMAGE[at + 11];
            root[i].time = u16::from_le_bytes([IMAGE[at + 22], IMAGE[at + 23]]);
            root[i].date = u16::from_le_bytes([IMAGE[at + 24], IMAGE[at + 25]]);
        }
        i += 1;
    }
    root
}
static ROOT: KernelCell<[Ent; SLOTS]> = KernelCell::new(image_root());

#[derive(Clone, Copy)]
pub(crate) struct Cwd {
    bytes: [u8; 32],
    len: usize,
}
impl Cwd {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
static CWD: KernelCell<Cwd> = KernelCell::new(Cwd {
    bytes: {
        let mut bytes = [0; 32];
        bytes[0] = b'A';
        bytes[1] = b':';
        bytes[2] = 0x5c;
        bytes
    },
    len: 3,
});
static DTA: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

struct Search {
    pattern: [u8; 11],
    attr: u8,
    next: usize,
    all: bool,
}
impl Search {
    const fn new() -> Self {
        Self {
            pattern: [b' '; 11],
            attr: 0,
            next: 0,
            all: false,
        }
    }
}
static SEARCH: KernelCell<Search> = KernelCell::new(Search::new());
static FCB_SEARCH: KernelCell<Search> = KernelCell::new(Search::new());

pub(crate) fn cwd_text() -> Cwd {
    *CWD.borrow()
}
pub(crate) fn set_cwd(text: &[u8]) {
    let mut cwd = CWD.borrow();
    let n = text.len().min(cwd.bytes.len());
    cwd.bytes[..n].copy_from_slice(&text[..n]);
    cwd.len = n;
}
pub(crate) fn cwd_is_root() -> bool {
    CWD.borrow().len == 3
}

pub(crate) fn walk(mut visit: impl FnMut(usize, Ent)) {
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
    walk(|_, ent| visit(ent.name));
}

pub(crate) fn slot(index: usize) -> Ent {
    ROOT.borrow()[index]
}
pub(crate) fn set_slot(index: usize, ent: Ent) {
    ROOT.borrow()[index] = ent;
}

pub(crate) fn find_packed(name: &[u8; 11]) -> Option<usize> {
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
    let mut index = 0usize;
    while index < SLOTS {
        if !slot(index).live {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Restore the previous native DTA when a boot command's local buffer goes out of scope.
pub(crate) struct SavedDta(*mut u8);
impl Drop for SavedDta {
    fn drop(&mut self) {
        set_dta(self.0);
    }
}
pub(crate) fn temporary_dta(ptr: *mut u8) -> SavedDta {
    SavedDta(DTA.swap(ptr, Ordering::Relaxed))
}

pub fn set_dta(ptr: *mut u8) {
    DTA.store(ptr, Ordering::Relaxed);
}

pub fn dta() -> *mut u8 {
    DTA.load(Ordering::Relaxed)
}

pub fn find_first(name: *const u8, attr: u8) -> u16 {
    let raw = read_asciiz(name);
    let text = &raw[..asciiz_len(&raw)];
    let all = matches!(text, b"*.*" | b"*");
    let pattern = if all {
        [b' '; 11]
    } else if let Some(pat) = pack(text) {
        pat
    } else {
        return NO_MORE;
    };
    *SEARCH.borrow() = Search {
        pattern,
        all,
        attr,
        next: 0,
    };
    find_next()
}

pub fn find_next() -> u16 {
    let mut search = SEARCH.borrow();
    while search.next < SLOTS {
        let index = search.next;
        search.next += 1;
        let ent = slot(index);
        if !visible(ent, search.attr)
            || !(search.all || ent.name.eq_ignore_ascii_case(&search.pattern))
        {
            continue;
        }
        write_dta(ent, index);
        return 0;
    }
    NO_MORE
}

fn visible(ent: Ent, sattr: u8) -> bool {
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
    file_attr & 0x1e & !sattr == 0
}

fn write_dta(ent: Ent, index: usize) {
    let dta = DTA.load(Ordering::Relaxed);
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

fn fcb_find(search: &Search) -> Option<usize> {
    let mut index = search.next;
    while index < SLOTS {
        let ent = slot(index);
        if ent.live && fcb_attr_ok(ent.attr, search.attr) && name_match(&search.pattern, &ent.name)
        {
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
    let (_, attr) = crate::dos::fcb::split(fcb);
    *FCB_SEARCH.borrow() = Search {
        pattern: crate::dos::fcb::packed_name(fcb),
        attr,
        ..Search::new()
    };
    fcb_search_next()
}

/// AH=12h.
pub fn fcb_search_next() -> u8 {
    let mut cursor = FCB_SEARCH.borrow();
    match fcb_find(&cursor) {
        Some(index) => {
            cursor.next = index + 1;
            write_fcb_dir(&slot(index), dta());
            0
        }
        None => 0xff,
    }
}
