//! `SYS_RET_ERR` and `CAL_LK` from `DOS/MS_CODE.ASM`.
//!
//! A failing call stores the error with AH clear, then looks the code up in
//! `ERR_TABLE_21`. Class, action, and locus are what AH=59h returns.

struct Row {
    code: u8,
    class: u8,
    action: u8,
    locus: u8,
}

/// The rows `PATH` and `ALLOC` actually return. `0xFF` means "leave the
/// previous locus alone".
const TABLE: &[Row] = &[
    Row { code: 1, class: 7, action: 4, locus: 0xff },
    Row { code: 2, class: 8, action: 3, locus: 2 },
    Row { code: 3, class: 8, action: 3, locus: 2 },
    Row { code: 5, class: 3, action: 3, locus: 0xff },
    Row { code: 6, class: 7, action: 4, locus: 1 },
    Row { code: 7, class: 7, action: 5, locus: 5 },
    Row { code: 8, class: 1, action: 4, locus: 5 },
    Row { code: 9, class: 7, action: 4, locus: 5 },
    Row { code: 15, class: 8, action: 3, locus: 2 },
    Row { code: 16, class: 3, action: 3, locus: 2 },
];

struct ExtendedError {
    code: u16,
    class: u8,
    action: u8,
    locus: u8,
}

static LAST: crate::kernel_cell::KernelCell<ExtendedError> =
    crate::kernel_cell::KernelCell::new(ExtendedError {
        code: 0,
        class: 0,
        action: 0,
        locus: 1,
    });

pub fn store(code: u16, class: u8, action: u8, locus: u8) {
    *LAST.borrow() = ExtendedError {
        code,
        class,
        action,
        locus,
    };
}

pub fn sys_ret_err(code: u16) -> u16 {
    let mut last = LAST.borrow();
    last.code = u16::from(code as u8);
    if let Some(row) = TABLE.iter().find(|row| row.code == code as u8) {
        last.class = row.class;
        last.action = row.action;
        if row.locus != 0xff {
            last.locus = row.locus;
        }
    }
    last.code
}

pub fn get() -> (u16, u16, u16) {
    let last = LAST.borrow();
    (
        last.code,
        u16::from_be_bytes([last.class, last.action]),
        u16::from(last.locus) << 8,
    )
}

pub fn show() {
    let mut buf = [0u8; 8];
    let _ = crate::dos::path::getcwd(buf.as_mut_ptr(), 2);
    let (status, handle, count) = query();
    crate::bios::mscon::con_write_bytes(b"ERR ");
    put_hex16(status);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((handle >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(handle as u8);
    crate::bios::mscon::con_write_bytes(b" LC");
    put_hex8((count >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn query() -> (u16, u16, u16) {
    get()
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
