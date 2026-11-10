//! Strings `BIOS/SYSIMES.ASM` publishes.
//!
//! `MSbio.cl3` was not in the pin. `BIOS/MSBIO.SKL` is in the pin and is
//! the skeleton that include is built from. These are the class 3 strings
//! that file names. No other message was added.

/// SKL def 03.
pub const BADOPM: &[u8] = b"\r\nUnrecognized command in CONFIG.SYS";

/// SKL def 04. `$` terminated. `BADFIL` uses this in place of `BADLD_POST`.
pub const CRLFM: &[u8] = b"\r\n$";

/// SKL def 22.
pub const BADPARM: &[u8] = b"\r\nBad command or parameters - $";

/// SKL def 05.
pub const BADSIZ_PRE: &[u8] = b"\r\nSector size too large in file $";

/// SKL def 06. `$` terminated.
pub const BADLD_PRE: &[u8] = b"\r\nBad or missing $";

/// SKL def 07. NUL terminated.
pub const BADCOM: &[u8] = b"Command Interpreter\0";

/// SKL def 08.
pub const BADCOUNTRY: &[u8] = b"\r\nInvalid country code or code page\r\n$";

/// SKL def 09.
pub const BADCOUNTRYCOM: &[u8] = b"\r\nError in COUNTRY command\r\n$";

/// SKL def 10.
pub const INSUFMEMORY: &[u8] = b"\r\nInsufficient memory for COUNTRY.SYS file\r\n$";

/// SKL def 11.
pub const BADMEM: &[u8] = b"\r\nConfiguration too large for memory\r\n$";

/// SKL def 12.
pub const BADBLOCK: &[u8] = b"\r\nToo many Block Devices\r\n$";

/// SKL def 13.
pub const BADSTACK: &[u8] = b"\r\nInvalid STACK parameters\r\n$";

/// SKL def 14.
pub const BADORDER: &[u8] = b"\r\nIncorrect order in CONFIG.SYS line $";

/// SKL def 15.
pub const ERRORCMD: &[u8] = b"Error in CONFIG.SYS line $";

/// SKL def 23.
pub const SHAREWARNMSG: &[u8] = b"WARNING! SHARE should be loaded for large media\r\n$";

/// Keep every string live. `COMERR` prints only three of them.
pub fn catalog_bytes() -> usize {
    BADOPM.len()
        + CRLFM.len()
        + BADPARM.len()
        + BADSIZ_PRE.len()
        + BADLD_PRE.len()
        + BADCOM.len()
        + BADCOUNTRY.len()
        + BADCOUNTRYCOM.len()
        + INSUFMEMORY.len()
        + BADMEM.len()
        + BADBLOCK.len()
        + BADSTACK.len()
        + BADORDER.len()
        + ERRORCMD.len()
        + SHAREWARNMSG.len()
}
