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

static mut EXTERR: u16 = 0;
static mut CLASS: u8 = 0;
static mut ACTION: u8 = 0;
static mut LOCUS: u8 = 1;

/// Store an extended error that was already mapped, including class, action, and locus.
pub fn store(code: u16, class: u8, action: u8, locus: u8) {
    unsafe {
        EXTERR = code;
        CLASS = class;
        ACTION = action;
        LOCUS = locus;
    }
}

/// `SYS_RET_ERR`. AH is cleared. The low byte is the error code.
pub fn sys_ret_err(code: u16) -> u16 {
    let al = (code & 0xff) as u8;
    unsafe { EXTERR = u16::from(al) }
    cal_lk();
    u16::from(al)
}

/// `CAL_LK` over `ERR_TABLE_21`.
fn cal_lk() {
    let code = unsafe { EXTERR as u8 };
    let mut index = 0usize;
    while index < TABLE.len() {
        if TABLE[index].code == code {
            unsafe {
                CLASS = TABLE[index].class;
                ACTION = TABLE[index].action;
                if TABLE[index].locus != 0xff {
                    LOCUS = TABLE[index].locus;
                }
            }
            return;
        }
        index += 1;
    }
}

/// `$GetExtendedError`. AX, then class in BH, action in BL, locus in CH.
pub fn get() -> (u16, u16, u16) {
    unsafe {
        let bx = (u16::from(CLASS) << 8) | u16::from(ACTION);
        let cx = u16::from(LOCUS) << 8;
        (EXTERR, bx, cx)
    }
}

pub fn show() {
    let mut buf = [0u8; 8];
    let _ = crate::dos::path::getcwd(buf.as_mut_ptr(), 2);
    let (ax, bx, cx) = query();
    crate::bios::mscon::con_write_bytes(b"ERR ");
    put_hex16(ax);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((bx >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(bx as u8);
    crate::bios::mscon::con_write_bytes(b" LC");
    put_hex8((cx >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn query() -> (u16, u16, u16) {
    let mut eax = 0x5900u32;
    let mut ecx = 0u32;
    let mut edx = 0u32;
    #[cfg(target_arch = "x86")]
    unsafe {
        core::arch::asm!(
            "push ebx",
            "xor ebx, ebx",
            "int 0x21",
            "mov edx, ebx",
            "pop ebx",
            inout("eax") eax,
            inout("ecx") ecx,
            inout("edx") edx,
            clobber_abi("C"),
        );
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor ebx, ebx",
            "int 0x21",
            "mov edx, ebx",
            "pop rbx",
            inout("eax") eax,
            inout("ecx") ecx,
            inout("edx") edx,
            clobber_abi("C"),
        );
    }
    (eax as u16, edx as u16, ecx as u16)
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
