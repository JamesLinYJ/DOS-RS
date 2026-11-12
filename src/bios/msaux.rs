//! AUX0 forwards writes to the COM1 backend used for BIOS serial output.

use super::msbio1::{self, DONE, Request, UNKNOWN_COMMAND};

pub fn aux_write_bytes(bytes: &[u8]) {
    msbio1::write(bytes, aux0_in);
}

fn aux0_in(request: &mut Request<'_>) {
    request.status = match request.command {
        8 | 9 => {
            for &byte in request.source.iter().take(usize::from(request.count)) {
                crate::serial_putc(byte);
            }
            DONE
        }
        _ => UNKNOWN_COMMAND,
    };
}
