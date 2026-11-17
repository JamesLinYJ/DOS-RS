//! Compile with rustc +stable --edition 2024 --test tests/native_fat_file.rs.
//! The production FAT driver runs against BIOS sectors and 20-bit DOS memory.
//! Each medium has two sectors per cluster, two FATs, and a FAT32 FSInfo backup.
#![allow(dead_code)]
use std::cell::RefCell;
const BUFFER: u16 = 0x3000;
const COMMAND: u16 = 0x3800;
#[path = "../boot/realmode/src/disk.rs"]
mod disk;
#[derive(Default)]
struct Mock {
    memory: Vec<u8>,
    disk: Vec<u8>,
    fail_write: Option<u32>,
}
thread_local! {static M:RefCell<Mock>=RefCell::new(Mock::default());}
fn byte(s: u16, p: u16) -> u16 {
    M.with(|m| m.borrow().memory[usize::from(s) * 16 + usize::from(p)] as u16)
}
fn store(s: u16, p: u16, v: u16) {
    M.with(|m| m.borrow_mut().memory[usize::from(s) * 16 + usize::from(p)] = v as u8)
}
fn word(p: u16) -> u16 {
    byte(BUFFER, p) | byte(BUFFER, p + 1) << 8
}
fn print(_: &str) {}
fn put(_: u16) {}
fn filename(_: u16, _: u16, _: u16) -> bool {
    false
}
fn copy_bytes(s: u16, p: u16, d: u16, q: u16, n: u16) {
    M.with(|m| {
        let mut x = m.borrow_mut();
        let i = usize::from(s) * 16 + usize::from(p);
        x.memory
            .copy_within(i..i + usize::from(n), usize::from(d) * 16 + usize::from(q))
    })
}
fn sector(lba: u32, write: bool) -> bool {
    M.with(|m| {
        let mut state = m.borrow_mut();
        let x = &mut *state;
        let at = usize::try_from(lba).unwrap() * 512;
        let b = usize::from(BUFFER) * 16;
        if at + 512 > x.disk.len() {
            return false;
        }
        if write {
            if x.fail_write == Some(lba) {
                x.fail_write = None;
                return false;
            }
            x.disk[at..at + 512].copy_from_slice(&x.memory[b..b + 512]);
        } else {
            x.memory[b..b + 512].copy_from_slice(&x.disk[at..at + 512]);
        }
        true
    })
}
fn wr16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn wr32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn f32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn volume(fat32: bool, free: usize) -> (disk::Volume, usize, usize) {
    let clusters = if fat32 { 65527 } else { 4094 };
    let entry = if fat32 { 4 } else { 2 };
    let fats = ((clusters + 2) * entry + 511) / 512;
    let res = if fat32 { 32 } else { 1 };
    let root = if fat32 { 0 } else { 2 };
    let data = res + 2 * fats + root;
    let sectors = data + clusters * 2;
    M.with(|m| {
        let mut x = m.borrow_mut();
        x.memory = vec![0; 1 << 20];
        x.disk = vec![0; sectors * 512];
        x.fail_write = None;
        let b = &mut x.disk;
        b[0] = 0xeb;
        wr16(b, 11, 512);
        b[13] = 2;
        wr16(b, 14, res as u16);
        b[16] = 2;
        wr32(b, 32, sectors as u32);
        wr16(b, 510, 0xaa55);
        if fat32 {
            wr32(b, 36, fats as u32);
            wr32(b, 44, 2);
            wr16(b, 48, 1);
            wr16(b, 50, 6);
            for si in [1, 7] {
                let a = si * 512;
                wr32(b, a, 0x41615252);
                wr32(b, a + 484, 0x61417272);
                wr32(b, a + 488, free as u32);
                wr32(b, a + 492, 3);
                wr32(b, a + 508, 0xaa550000)
            }
        } else {
            wr16(b, 17, 32);
            wr16(b, 22, fats as u16);
        }
        for t in 0..2 {
            let a = (res + t * fats) * 512;
            for i in 0..clusters + 2 {
                let begin = if fat32 { 3 } else { 2 };
                if i < begin || i >= begin + free {
                    if fat32 {
                        wr32(b, a + i * 4, 0xfffffff)
                    } else {
                        wr16(b, a + i * 2, 0xffff)
                    }
                }
            }
        }
    });
    let mut v = disk::Volume::EMPTY;
    assert!(v.mount());
    (v, data, fats)
}
fn open(v: &mut disk::Volume, n: &[u8; 11]) -> (u32, u16) {
    for (i, b) in n.iter().enumerate() {
        store(COMMAND, 128 + i as u16, *b as u16)
    }
    let mut l = 0;
    let mut a = 0;
    assert_eq!(v.file_open(true, 0, &mut l, &mut a), 0);
    (l, a)
}
fn stat(v: &disk::Volume, id: (u32, u16)) -> (u32, u32) {
    let (mut f, mut s, mut a) = (0, 0, 0);
    assert_eq!(v.file_stat(id.0, id.1, &mut f, &mut s, &mut a), 0);
    (f, s)
}
fn check(v: &disk::Volume, id: (u32, u16), want: &[u8]) {
    let (f, s) = stat(v, id);
    assert_eq!(s, want.len() as u32);
    let mut p = 0;
    let mut all = Vec::new();
    while p < s {
        let n = v.read_at(f, s, p, 20000, 0x4000, 321);
        assert!(n > 0 && n <= 20000);
        all.extend((0..n).map(|i| byte(0x4000, 321 + i as u16) as u8));
        p += n;
    }
    assert_eq!(all, want);
}
fn write(v: &mut disk::Volume, id: (u32, u16), pos: u32, data: &[u8]) -> u32 {
    for (i, x) in data.iter().enumerate() {
        store(0x5000, i as u16, *x as u16)
    }
    v.file_write(id.0, id.1, pos, data.len() as u16, 0x5000, 0)
}
fn tables(f32_: bool, fats: usize, free: u32) {
    M.with(|m| {
        let x = m.borrow();
        let res = if f32_ { 32 } else { 1 };
        let a = res * 512;
        assert_eq!(
            &x.disk[a..a + fats * 512],
            &x.disk[a + fats * 512..a + 2 * fats * 512]
        );
        if f32_ {
            for t in [1, 7] {
                assert_eq!(f32(&x.disk, t * 512 + 488), free)
            }
        }
    });
}
fn exercise(is32: bool) {
    let (mut v, _, fats) = volume(is32, 430);
    let id = open(&mut v, b"MAIN    BIN");
    let other = open(&mut v, b"OTHER   BIN");
    let z: Vec<u8> = (0..1400).map(|x| (x * 27) as u8).collect();
    let mut expect = vec![];
    for (p, n) in [
        (100_001, 1200),
        (65331, 1200),
        (0, 1200),
        (99_999, 10),
        (134669, 1400),
        (120, 900),
        (8000, 1400),
    ] {
        assert_eq!(write(&mut v, id, p, &z[..n]), n as u32);
        let end = p as usize + n;
        if expect.len() < end {
            expect.resize(end, 0)
        }
        expect[p as usize..end].copy_from_slice(&z[..n]);
        check(&v, id, &expect);
    }
    assert_eq!(write(&mut v, other, 23, &z[..100]), 100);
    let mut w = vec![0; 23];
    w.extend(&z[..100]);
    check(&v, other, &w);
    check(&v, id, &expect);
    for end in [65536, 777, 1024, 129_111, 0, 1000, 0] {
        assert_eq!(v.file_resize(id.0, id.1, end), 0);
        expect.resize(end as usize, 0);
        check(&v, id, &expect)
    }
    tables(is32, fats, 429);
    let (mut v, _, fats) = volume(is32, 3);
    let id = open(&mut v, b"LOW     BIN");
    let res = if is32 { 32 } else { 1 };
    M.with(|m| m.borrow_mut().fail_write = Some((res + fats) as u32));
    assert!(write(&mut v, id, 0, b"failed") >= 0x10000);
    check(&v, id, &[]);
    tables(is32, fats, 3);
    if is32 {
        M.with(|m| m.borrow_mut().fail_write = Some(7));
        assert_eq!(write(&mut v, id, 0, b"ok"), 2);
        check(&v, id, b"ok");
        tables(is32, fats, 2);
        assert_eq!(v.file_resize(id.0, id.1, 0), 0);
        tables(is32, fats, 3);
    }

    assert_eq!(write(&mut v, id, 8000, b"!"), 0);
    check(&v, id, &[]);
    tables(is32, fats, 3);
    assert_eq!(v.file_resize(id.0, id.1, 8000), 39);
    check(&v, id, &[]);
    tables(is32, fats, 3);
    let seq = vec![9; 4000];
    assert_eq!(write(&mut v, id, 700, &seq), 2372);
    let mut want = vec![0; 700];
    want.resize(3072, 9);
    check(&v, id, &want);
    tables(is32, fats, 0);
    assert_eq!(write(&mut v, id, 3072, b"!"), 0);
    check(&v, id, &want);
    assert_eq!(v.file_resize(id.0, id.1, 0), 0);
    tables(is32, fats, 3);
    let (mut v, base, fats) = volume(is32, 3);
    let id = open(&mut v, b"FAULT   BIN");
    let first_free = if is32 { 3 } else { 2 };
    let start_lba = base + (first_free - 2) * 2;
    M.with(|m| m.borrow_mut().fail_write = Some(start_lba as u32 + 1));
    assert_eq!(write(&mut v, id, 0, &seq[..1400]), 512);
    check(&v, id, &seq[..512]);
    tables(is32, fats, 2);
    M.with(|m| m.borrow_mut().fail_write = Some(id.0));
    assert_eq!(write(&mut v, id, 500, &seq[..800]), 12);
    check(&v, id, &seq[..512]);
    tables(is32, fats, 2);

    // Read-only is assigned to the file at creation. Permissions on the open
    // SFT, including that initial writable handle, are checked by DOS.
    let mut file_lba = 0;
    let mut file_at = 0;
    for (i, ch) in b"RO      TXT".iter().enumerate() {
        store(COMMAND, 128 + i as u16, u16::from(*ch));
    }
    assert_eq!(v.file_open(true, 1, &mut file_lba, &mut file_at), 0);
    assert_eq!(write(&mut v, (file_lba, file_at), 0, b"R"), 1);
    assert_eq!(v.file_resize(file_lba, file_at, 0), 0);
    assert_eq!(v.file_open(true, 1, &mut file_lba, &mut file_at), 5);
    let (mut head, mut size, mut flags) = (0, 0, 0);
    assert_eq!(
        v.file_stat(file_lba, file_at, &mut head, &mut size, &mut flags),
        0
    );
    assert_eq!(flags & 1, 1);
}
#[test]
fn fat16_file_ranges() {
    exercise(false)
}
#[test]
fn fat32_file_ranges() {
    exercise(true)
}
