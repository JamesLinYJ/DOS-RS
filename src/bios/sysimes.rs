//! Strings `BIOS/SYSIMES.ASM` publishes for the missing command interpreter.
//!
//! `MSbio.cl3` was not in the pin. `BIOS/MSBIO.SKL` is in the pin and is
//! the skeleton that include is built from. `BADFIL` for `COMERR` uses
//! three of those strings and no others.

/// SKL def 06. `$` terminated.
pub const BADLD_PRE: &[u8] = b"\r\nBad or missing $";

/// SKL def 07. NUL terminated.
pub const BADCOM: &[u8] = b"Command Interpreter\0";

/// SKL def 04. `$` terminated. `BADFIL` uses this in place of `BADLD_POST`.
pub const CRLFM: &[u8] = b"\r\n$";
