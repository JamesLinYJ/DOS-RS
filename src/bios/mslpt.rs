//! Printer driver from `BIOS/MSLPT.ASM`.
//!
//! `PRN$WRIT` retries only when INT 17h reports timeout. `PRN$STAT` uses
//! AH=2. Under QEMU `-kernel` there is no printer ROM. AH=0 writes AL to
//! COM1 and returns AH=0. AH=2 returns AH=80h, not busy and no error.
//! `WAIT_COUNT` starts at 50h for each of the four devices.

const STATUS: usize = 3;
const MAJOR: usize = 13;
const MINOR: usize = 14;
const IOCTL_PACKET: usize = 19;
const DONE: u16 = 0x0100;
const BUSY_DONE: u16 = 0x0300;
const ERROR_DONE: u16 = 0x8100;
const IOERR: u8 = 0x08;
const TIMEOUT: u8 = 0x01;
const NOPAPER: u8 = 0x20;
const NOT_BUSY: u8 = 0x80;
const IOC_PC: u8 = 5;
const GET_RETRY: u8 = 0x65;
const SET_RETRY: u8 = 0x45;

static mut PRINTDEV: u8 = 0;
static mut WAIT_COUNT: [u16; 4] = [0x50, 0x50, 0x50, 0x50];

pub fn prn_write_bytes(bytes: &[u8]) -> u16 {
    if bytes.is_empty() {
        return DONE;
    }
    let mut index = 0usize;
    while index < bytes.len() {
        let mut retries = 2u16;
        loop {
            let (function, zf) = prn_op(0, bytes[index]);
            if zf || function & TIMEOUT == 0 {
                break;
            }
            retries -= 1;
            if retries == 0 {
                return ERROR_DONE | 2;
            }
        }
        index += 1;
    }
    DONE
}

/// `PRN$STAT`. Not busy returns done. Busy returns the busy-done status.
pub fn prn_stat() -> u16 {
    let (function, zf) = prn_op(2, 0);
    if !zf {
        let result = if function & NOPAPER != 0 { 9 } else { 10 };
        return ERROR_DONE | result;
    }
    if function & NOT_BUSY != 0 {
        DONE
    } else {
        BUSY_DONE
    }
}

/// `PRN$TILBUSY`. Stops when the status says the printer is still busy.
pub fn prn_tilbusy(bytes: &[u8]) -> u16 {
    let mut done = 0u16;
    for &byte in bytes {
        let (function, zf) = prn_op(2, 0);
        if !zf || function & NOT_BUSY == 0 {
            break;
        }
        let (function, zf) = prn_op(0, byte);
        if !zf {
            let _ = function;
            break;
        }
        done += 1;
    }
    done
}

/// `PRN$GENIOCTL` get-retry. The packet is the `IOCTL_REQ` layout.
pub fn prn_ioctl_get() -> u16 {
    let mut data = [0u8; 2];
    let mut packet = [0u8; 32];
    packet[MAJOR] = IOC_PC;
    packet[MINOR] = GET_RETRY;
    let addr = data.as_mut_ptr() as u32;
    packet[IOCTL_PACKET..IOCTL_PACKET + 4].copy_from_slice(&addr.to_le_bytes());
    prn_genioctl(packet.as_mut_ptr());
    u16::from_le_bytes(data)
}

fn prn_genioctl(packet: *mut u8) {
    let major = unsafe { *packet.add(MAJOR) };
    if major != IOC_PC {
        write_word(packet, STATUS, ERROR_DONE | 3);
        return;
    }
    let minor = unsafe { *packet.add(MINOR) };
    let data = read_dword(packet, IOCTL_PACKET) as *mut u8;
    let index = unsafe { PRINTDEV } as usize & 3;
    if minor == GET_RETRY {
        let count = unsafe { WAIT_COUNT[index] };
        write_word(data, 0, count);
        write_word(packet, STATUS, DONE);
    } else if minor == SET_RETRY {
        let count = read_word(data, 0);
        unsafe { WAIT_COUNT[index] = count };
        write_word(data, 0, count);
        write_word(packet, STATUS, DONE);
    } else {
        write_word(packet, STATUS, ERROR_DONE | 3);
    }
}

/// `PRNOP` / `PRNSTAT`. Returns AH and ZF from the status triage.
fn prn_op(ah_cmd: u8, result: u8) -> (u8, bool) {
    let _dx = super::msbio1::get_dx();
    let function = int17(ah_cmd, result);
    if function & IOERR != 0 {
        let zf = function & NOPAPER == 0;
        return (function, zf);
    }
    let zf = function & TIMEOUT == 0;
    (function, zf)
}

fn int17(command: u8, byte: u8) -> u8 {
    printer(command, byte)
}

fn printer(command: u8, byte: u8) -> u8 {
    if command == 0 {
        crate::serial_putc(byte);
        0
    } else if command == 2 {
        NOT_BUSY
    } else {
        0
    }
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}

fn read_word(pkt: *const u8, offset: usize) -> u16 {
    unsafe { u16::from_le_bytes([*pkt.add(offset), *pkt.add(offset + 1)]) }
}

fn read_dword(pkt: *const u8, offset: usize) -> u32 {
    unsafe {
        u32::from_le_bytes([
            *pkt.add(offset),
            *pkt.add(offset + 1),
            *pkt.add(offset + 2),
            *pkt.add(offset + 3),
        ])
    }
}
