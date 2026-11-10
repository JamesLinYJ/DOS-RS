//! Default environment from `CMD/COMMAND/ENVDATA.ASM`.
//!
//! The block is `PATH=`, an empty path, `COMSPEC=\COMMAND.COM`, then 134
//! zero bytes. `ECOMSPEC` is the tail the exec payload carries.

pub const PATHSTRING: &[u8] = b"PATH=";
pub const COMSPEC_KEY: &[u8] = b"COMSPEC=";
pub const ECOMSPEC: &[u8] = b"\\COMMAND.COM";
const PAD: usize = 134;

pub fn environ_len() -> usize {
    PATHSTRING.len() + 1 + COMSPEC_KEY.len() + ECOMSPEC.len() + PAD
}

pub fn payload_is_comspec(payload: &[u8]) -> bool {
    payload.starts_with(ECOMSPEC)
}
