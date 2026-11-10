//! Share-level `PFMT` from `DOS/SHRPRINT.ASM`.
//!
//! The formatter is the same `PRINT.ASM` routine, assembled into the SHARE
//! segment. `$p` prints the user id, a colon, and the process id.

pub fn show() {
    let mut raw = [0u8; 16];
    let count = crate::dos::dosprint::format(b"$p", &[], 1, 2, &mut raw);
    crate::bios::mscon::con_write_bytes(b"SHRPRINT ");
    crate::bios::mscon::con_write_bytes(&raw[..count]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}
