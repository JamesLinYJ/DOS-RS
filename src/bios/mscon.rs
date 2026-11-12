//! DOS console device. Teletype output goes to COM1. The boot machine has
//! no keyboard backend, so a nonblocking input request reports busy.

use super::msbio1::{self, DONE, Request, UNKNOWN_COMMAND};

pub fn con_write_bytes(bytes: &[u8]) {
    msbio1::write(bytes, con_in);
}

fn con_in(request: &mut Request<'_>) {
    let count = usize::from(request.count);
    request.status = match request.command {
        0..=2 | 6 | 7 | 10 => DONE,
        4 => {
            for dest in request.destination.iter_mut().take(count) {
                *dest = loop {
                    if let Some(byte) = keyboard_byte() {
                        break byte;
                    }
                    core::hint::spin_loop();
                };
            }
            DONE
        }
        5 => match keyboard_byte() {
            Some(byte) => {
                request.media = byte;
                DONE | u16::from(byte)
            }
            None => 0x0300,
        },
        8 | 9 => {
            for &byte in request.source.iter().take(count) {
                if byte != 0 {
                    crate::serial_putc(byte);
                }
            }
            DONE
        }
        _ => {
            request.count = request.count.wrapping_neg();
            UNKNOWN_COMMAND
        }
    };
}

fn keyboard_byte() -> Option<u8> {
    None
}

pub(crate) fn write_number(mut value: u8) {
    let mut digits = [0; 3];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + value % 10;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    con_write_bytes(&digits[start..]);
}
