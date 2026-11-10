//! Console driver from `BIOS/MSCON.ASM`, plus `OUTCHR` from `BIOS/MSBIO1.ASM`.
//!
//! `CON$WRIT` executes `int 0x29` once per byte. `OUTCHR` is that interrupt.
//! It executes `int 0x10` with AH=`0x0E` and BX=7. There is no VGA BIOS under
//! QEMU `-kernel`, so AH=`0x0E` writes AL to COM1. Other AH values return.
//! There is no ROM keyboard, so `int 0x16` returns AX=0 with ZF set.

const CMDLEN: usize = 0;
const UNIT: usize = 1;
const CMD: usize = 2;
const STATUS: usize = 3;
const MEDIA: usize = 13;
const TRANS: usize = 14;
const COUNT: usize = 18;

const DEV_READ: u8 = 4;
const DEV_RDND: u8 = 5;
const DEV_FLUSH: u8 = 7;
const DEV_WRITE: u8 = 8;
const DEV_WRITE_VERIFY: u8 = 9;

const DONE: u16 = 0x0100;
const BUSY_DONE: u16 = 0x0300;
const ERROR_DONE: u16 = 0x8100;
const UNKNOWN_COMMAND: u8 = 3;
const SYSTEM_WAIT: u16 = 0x0400;

struct ConState {
    altah: u8,
    keyrd_func: u8,
    keysts_func: u8,
    fhavek09: u8,
}

static mut CON: ConState = ConState {
    altah: 0,
    keyrd_func: 0,
    keysts_func: 1,
    fhavek09: 0,
};

/// Maximum command in `CONTBL` (`DB 10`), then one slot per command.
const CONTBL: [fn(); 11] = [
    exit_ok,
    exit_ok,
    exit_ok,
    cmd_err,
    con_read,
    con_rdnd,
    exit_ok,
    con_flush,
    con_writ,
    con_writ,
    exit_ok,
];

const _: () = assert!(CONTBL.len() - 1 == 10);

#[inline(never)]
pub fn con_write_bytes(bytes: &[u8]) {
    let mut packet = [0u8; 32];
    packet[CMDLEN] = 22;
    packet[CMD] = DEV_WRITE;
    let addr = bytes.as_ptr() as u32;
    packet[TRANS..TRANS + 4].copy_from_slice(&addr.to_le_bytes());
    let count = bytes.len() as u16;
    packet[COUNT..COUNT + 2].copy_from_slice(&count.to_le_bytes());
    super::msbio1::run_con(packet.as_mut_ptr());
}

pub(crate) fn con_in() {
    let cmd = packet_byte(CMD);
    if cmd as usize >= CONTBL.len() {
        cmd_err();
        return;
    }
    CONTBL[cmd as usize]();
}

fn con_read() {
    let count = packet_word(COUNT);
    if count == 0 {
        exit_ok();
        return;
    }
    let dest = trans_ptr() as *mut u8;
    let mut left = count;
    while left != 0 {
        let result = chrin();
        unsafe {
            dest.add((count - left) as usize).write(result);
        }
        left -= 1;
    }
    exit_ok();
}

fn chrin() -> u8 {
    loop {
        let func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let held = unsafe {
            let state = &mut *core::ptr::addr_of_mut!(CON);
            let result = state.altah;
            state.altah = 0;
            result
        };
        if held != 0 {
            return held;
        }
        let (status, _zf) = int16(func);
        if status == 0 {
            continue;
        }
        if status == 0x7200 {
            return 16;
        }
        let result = status as u8;
        let function = (status >> 8) as u8;
        if func != 0 && result == 0xE0 {
            if function == 0 {
                return result;
            }
            unsafe {
                (*core::ptr::addr_of_mut!(CON)).altah = function;
            }
            return 0;
        }
        if result != 0 {
            return result;
        }
        unsafe {
            (*core::ptr::addr_of_mut!(CON)).altah = function;
        }
        return 0;
    }
}

fn con_rdnd() {
    let held = unsafe { (*core::ptr::addr_of!(CON)).altah };
    if held != 0 {
        rd_exit(held);
        return;
    }
    let func = unsafe { (*core::ptr::addr_of!(CON)).keysts_func };
    let (status, zf) = int16(func);
    if zf {
        no_char();
        return;
    }
    if status == 0 {
        let read_func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let _ = int16(read_func);
        con_rdnd();
        return;
    }
    if status == 0x7200 {
        rd_exit(16);
        return;
    }
    let mut result = status as u8;
    let function = (status >> 8) as u8;
    let extended = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func } != 0;
    if extended && result == 0xE0 && function != 0 {
        result = 0;
    }
    rd_exit(result);
}

fn no_char() {
    let k09 = unsafe { (*core::ptr::addr_of!(CON)).fhavek09 };
    if k09 == 0 {
        bus_exit();
        return;
    }
    let status = packet_word(STATUS);
    if status & SYSTEM_WAIT == 0 {
        bus_exit();
        return;
    }
    bus_exit();
}

fn rd_exit(result: u8) {
    let pkt = ptrsav();
    unsafe {
        *pkt.add(MEDIA) = result;
    }
    exit_with(result);
}

fn con_flush() {
    unsafe {
        (*core::ptr::addr_of_mut!(CON)).altah = 0;
    }
    loop {
        let func = unsafe { (*core::ptr::addr_of!(CON)).keysts_func };
        let (_ax, zf) = int16(func);
        if zf {
            break;
        }
        let read_func = unsafe { (*core::ptr::addr_of!(CON)).keyrd_func };
        let _ = int16(read_func);
    }
    exit_ok();
}

fn con_writ() {
    let count = packet_word(COUNT);
    if count == 0 {
        exit_ok();
        return;
    }
    let src = trans_ptr();
    let mut left = count;
    while left != 0 {
        let result = unsafe { *src.add((count - left) as usize) };
        int29(result);
        left -= 1;
    }
    exit_ok();
}

fn exit_ok() {
    exit_with(0);
}

fn cmd_err() {
    let pkt = ptrsav();
    let count = packet_word(COUNT);
    write_word(pkt, COUNT, 0u16.wrapping_sub(count));
    write_word(pkt, STATUS, ERROR_DONE | UNKNOWN_COMMAND as u16);
}

fn bus_exit() {
    let pkt = ptrsav();
    write_word(pkt, STATUS, BUSY_DONE);
}

fn exit_with(result: u8) {
    let pkt = ptrsav();
    write_word(pkt, STATUS, DONE | result as u16);
}

fn ptrsav() -> *mut u8 {
    super::msbio1::packet()
}

fn packet_byte(offset: usize) -> u8 {
    unsafe { *ptrsav().add(offset) }
}

fn packet_word(offset: usize) -> u16 {
    let pkt = ptrsav();
    unsafe { u16::from_le_bytes([*pkt.add(offset), *pkt.add(offset + 1)]) }
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}

fn trans_ptr() -> *const u8 {
    let pkt = ptrsav();
    let mut raw = [0u8; 4];
    unsafe {
        raw[0] = *pkt.add(TRANS);
        raw[1] = *pkt.add(TRANS + 1);
        raw[2] = *pkt.add(TRANS + 2);
        raw[3] = *pkt.add(TRANS + 3);
    }
    u32::from_le_bytes(raw) as *const u8
}

fn int29(byte: u8) {
    teletype(byte);
}

fn int16(_command: u8) -> (u16, bool) {
    (0, true)
}

#[allow(dead_code)]
fn bios_outchr(byte: usize) {
    teletype(byte as u8);
}

fn teletype(byte: u8) {
    crate::serial_putc(byte);
}

#[allow(dead_code)]
fn _command_names() {
    let _ = (DEV_READ, DEV_RDND, DEV_FLUSH, DEV_WRITE_VERIFY, UNIT);
}
