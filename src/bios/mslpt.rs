//! Printer driver from `BIOS/MSLPT.ASM`.
//!
//! `PRN$WRIT` retries only when INT 17h reports timeout. `PRN$STAT` uses
//! AH=2. Under QEMU `-kernel` there is no printer ROM. AH=0 writes AL to
//! COM1 and returns AH=0. AH=2 returns AH=80h, not busy and no error.
//! `WAIT_COUNT` starts at 50h for each of the four devices.

use core::sync::atomic::{AtomicU16, Ordering};

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

static WAIT_COUNT: [AtomicU16; 4] = [const { AtomicU16::new(0x50) }; 4];

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

/// Read the retry count for the default printer.
pub fn prn_ioctl_get() -> u16 {
    let mut count = 0;
    prn_genioctl(0, IOC_PC, GET_RETRY, &mut count);
    count
}

fn prn_genioctl(device: usize, major: u8, minor: u8, count: &mut u16) -> u16 {
    if major != IOC_PC {
        return ERROR_DONE | 3;
    }
    let retries = &WAIT_COUNT[device & 3];
    match minor {
        GET_RETRY => *count = retries.load(Ordering::Relaxed),
        SET_RETRY => retries.store(*count, Ordering::Relaxed),
        _ => return ERROR_DONE | 3,
    }
    DONE
}

/// `PRNOP` / `PRNSTAT`. Returns AH and ZF from the status triage.
fn prn_op(ah_cmd: u8, result: u8) -> (u8, bool) {
    let function = printer(ah_cmd, result);
    if function & IOERR != 0 {
        let zf = function & NOPAPER == 0;
        return (function, zf);
    }
    let zf = function & TIMEOUT == 0;
    (function, zf)
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
