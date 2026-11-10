//! Auxiliary driver write path from `BIOS/MSAUX.ASM`.
//!
//! `AUX$WRIT` sends each byte with INT 14h AH=1. There is no ROM serial BIOS
//! under QEMU `-kernel`. AH=1 writes AL to COM1 and leaves bit 7 of AH clear,
//! which is the no-timeout status `AUX$WRIT` checks.

const CMD: usize = 2;
const STATUS: usize = 3;
const TRANS: usize = 14;
const COUNT: usize = 18;
const DEV_WRITE: u8 = 8;
const DONE: u16 = 0x0100;
const ERROR_DONE: u16 = 0x8100;
const WRITE_FAULT: u8 = 10;
const FLAG_TIMEOUT: u8 = 0x80;

#[inline(never)]
pub fn aux_write_bytes(bytes: &[u8]) {
    let mut packet = [0u8; 32];
    packet[0] = 22;
    packet[CMD] = DEV_WRITE;
    let addr = bytes.as_ptr() as u32;
    packet[TRANS..TRANS + 4].copy_from_slice(&addr.to_le_bytes());
    let count = bytes.len() as u16;
    packet[COUNT..COUNT + 2].copy_from_slice(&count.to_le_bytes());
    super::msbio1::run_aux0(packet.as_mut_ptr());
}

pub(crate) fn aux0_in() {
    let cmd = unsafe { *ptrsav().add(CMD) };
    if cmd == DEV_WRITE || cmd == DEV_WRITE + 1 {
        aux_writ();
    } else {
        write_word(ptrsav(), STATUS, ERROR_DONE | 3);
    }
}

fn aux_writ() {
    let count = packet_word(COUNT);
    if count == 0 {
        exit_ok();
        return;
    }
    let src = trans_ptr();
    let mut left = count;
    while left != 0 {
        let al = unsafe { *src.add((count - left) as usize) };
        let ah = int14_send(al);
        if ah & FLAG_TIMEOUT != 0 {
            let pkt = ptrsav();
            write_word(pkt, COUNT, count - left);
            write_word(pkt, STATUS, ERROR_DONE | WRITE_FAULT as u16);
            return;
        }
        left -= 1;
    }
    exit_ok();
}

fn int14_send(byte: u8) -> u8 {
    let _port = super::msbio1::get_dx();
    crate::serial_putc(byte);
    0x01
}

fn exit_ok() {
    write_word(ptrsav(), STATUS, DONE);
}

fn ptrsav() -> *mut u8 {
    super::msbio1::packet()
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

