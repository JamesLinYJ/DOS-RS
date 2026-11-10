//! Internal file open from `DOS/OPEN.ASM`.
//!
//! Access is checked before the CDS. A null CDS or a network CDS issues
//! redirector verb 22, or verb 46 when extended open is on. A local directory
//! or volume is rejected. A read-only file forces an FCB or net-FCB open down
//! to read, and rejects any other write. Sharing above 40h is invalid unless
//! the server passed a net FCB.

const ACCESS_MASK: u8 = 0x0f;
const OPEN_FOR_READ: u8 = 0x00;
const OPEN_FOR_WRITE: u8 = 0x01;
const SHARING_MASK: u8 = 0xf0;
const SHARING_COMPAT: u8 = 0x00;
const SHARING_DENY_NONE: u8 = 0x40;
const SHARING_NET_FCB: u8 = 0x70;
const SF_IS_FCB: u16 = 0x8000;
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_VOLUME: u8 = 0x08;
const ERROR_FILE_NOT_FOUND: u16 = 2;
const ERROR_PATH_NOT_FOUND: u16 = 3;
const ERROR_ACCESS_DENIED: u16 = 5;
const ERROR_INVALID_ACCESS: u16 = 12;
const ERROR_SHARING_VIOLATION: u16 = 32;
const FAST_OPEN_SET: u8 = 0x01;
const SPECIAL_FILL_SET: u8 = 0x04;
const FAST_YES: u8 = 0x80;
const VERB_OPEN: u8 = 22;
const VERB_EXTENDED: u8 = 46;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShareReply {
    Retry,
    Fail,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cds {
    Null,
    Network,
    Local,
}

#[derive(Clone, Copy)]
pub struct Request {
    pub mode: u8,
    pub server: bool,
    pub fcb: bool,
    pub extended: bool,
    pub process: u16,
}

#[derive(Clone, Copy)]
pub struct ShareTry {
    pub tries: u16,
    pub failures: u16,
    pub user_retries: u16,
}

#[derive(Clone, Copy)]
pub enum Lookup {
    Hit {
        directory: bool,
        device: bool,
        attr: u8,
    },
    Miss {
        final_name: bool,
        final_len: u8,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Remote {
        verb: u8,
        extended_cleared: bool,
    },
    Local {
        mode: u16,
        process: Option<u16>,
        fast_flags: u8,
    },
}

/// `Check_Access_AX`. A server net FCB skips the sharing-range test.
pub fn check_access(mode: u8, server: bool) -> Result<u8, u16> {
    let sharing = mode & SHARING_MASK;
    let sharing_ok = if server && sharing == SHARING_NET_FCB {
        true
    } else {
        sharing <= SHARING_DENY_NONE
    };
    if !sharing_ok {
        return Err(ERROR_INVALID_ACCESS);
    }
    let access = mode & ACCESS_MASK;
    if access > 2 {
        return Err(ERROR_INVALID_ACCESS);
    }
    Ok(mode)
}

/// `SHARE_ERROR`. FCB and compatibility opens ask the critical-error handler.
pub fn share_error(mode: u16, reply: ShareReply) -> Result<(), u16> {
    let fcb = mode & SF_IS_FCB != 0;
    let sharing = (mode as u8) & SHARING_MASK;
    if fcb || sharing == SHARING_COMPAT {
        return match reply {
            ShareReply::Retry => Ok(()),
            ShareReply::Fail => Err(ERROR_SHARING_VIOLATION),
        };
    }
    Err(ERROR_SHARING_VIOLATION)
}

/// `DO_SHARE_CHECK`. `tries` attempts, then `SHARE_ERROR` may start them over.
pub fn do_share_check(
    mode: u16,
    tries: u16,
    mut failures: u16,
    mut user_retries: u16,
) -> Result<(), u16> {
    loop {
        if share_round(tries, &mut failures) {
            return Ok(());
        }
        let reply = if user_retries > 0 {
            user_retries -= 1;
            ShareReply::Retry
        } else {
            ShareReply::Fail
        };
        share_error(mode, reply)?;
    }
}

/// `OWN_SHARE`. The installed body returns without inspecting the IFS header.
pub fn own_share() {}

/// `OWN_SHARE2`. The installed body returns without inspecting the CDS.
pub fn own_share2() {}

/// `SET_THISDPB`. The installed body returns without storing a DPB.
pub fn set_this_dpb() {}

/// `IFS_SHARE_CHECK`. The installed body returns without calling share.
pub fn ifs_share_check() {}

/// `Code_Page_Mismatched_Error`. The file-tagging body is not assembled.
pub fn code_page_mismatched() {}

/// `SET_SFT_MODE`. An FCB records the current process. Any other open does not.
pub fn set_sft_mode(mode: u16, process: u16) -> Option<u16> {
    if mode & SF_IS_FCB == 0 {
        return None;
    }
    Some(process)
}

/// `DOS_Open`.
pub fn dos_open(
    request: Request,
    cds: Cds,
    lookup: Lookup,
    share: ShareTry,
    fast_flags: u8,
) -> Result<Opened, u16> {
    let mode_byte = check_access(request.mode, request.server)?;
    let mut mode = u16::from(mode_byte);
    if request.fcb {
        mode |= SF_IS_FCB;
    }
    if let Cds::Null = cds {
        return Ok(remote(request.extended));
    }
    if let Cds::Network = cds {
        return Ok(remote(request.extended));
    }
    let local = finish_local(mode, fast_flags, lookup);
    if let Some(error) = local.error {
        return Err(error);
    }
    do_share_check(
        local.mode,
        share.tries,
        share.failures,
        share.user_retries,
    )?;
    Ok(Opened::Local {
        mode: local.mode,
        process: set_sft_mode(local.mode, request.process),
        fast_flags: local.fast_flags,
    })
}

pub fn show() {
    own_share();
    own_share2();
    set_this_dpb();
    ifs_share_check();
    code_page_mismatched();

    let accepted = check_access(0x00, false);
    let bad_share = check_access(0x50, false);
    let bad_access = check_access(0x03, false);
    let net_fcb = check_access(SHARING_NET_FCB, true);
    let net_access = check_access(SHARING_NET_FCB | 0x03, true);
    let plain_net = check_access(SHARING_NET_FCB, false);
    let quiet = ShareTry {
        tries: 3,
        failures: 0,
        user_retries: 0,
    };
    let blocked = dos_open(
        request(0x50, false, false, false),
        Cds::Null,
        file(0),
        quiet,
        0,
    );
    let readonly_write = dos_open(
        request(OPEN_FOR_WRITE, false, false, false),
        Cds::Local,
        file(ATTR_READ_ONLY),
        quiet,
        0,
    );
    let readonly_read = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        file(ATTR_READ_ONLY),
        quiet,
        0,
    );
    let fcb_write = dos_open(
        request(OPEN_FOR_WRITE, false, true, false),
        Cds::Local,
        file(ATTR_READ_ONLY),
        quiet,
        0,
    );
    let net_fcb_write = dos_open(
        request(SHARING_NET_FCB | OPEN_FOR_WRITE, true, false, false),
        Cds::Local,
        file(ATTR_READ_ONLY),
        quiet,
        0,
    );
    let directory = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        Lookup::Hit {
            directory: true,
            device: true,
            attr: 0,
        },
        quiet,
        0,
    );
    let volume = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        file(ATTR_VOLUME),
        quiet,
        0,
    );
    let device = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        Lookup::Hit {
            directory: false,
            device: true,
            attr: ATTR_VOLUME,
        },
        quiet,
        0xff,
    );
    let not_found = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        Lookup::Miss {
            final_name: true,
            final_len: 1,
        },
        quiet,
        0,
    );
    let bad_path = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        Lookup::Miss {
            final_name: false,
            final_len: 1,
        },
        quiet,
        0,
    );
    let empty_tail = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Local,
        Lookup::Miss {
            final_name: true,
            final_len: 0,
        },
        quiet,
        0,
    );
    let sequential = dos_open(
        request(OPEN_FOR_READ, false, false, false),
        Cds::Null,
        file(0),
        quiet,
        0,
    );
    let extended = dos_open(
        request(OPEN_FOR_READ, false, false, true),
        Cds::Network,
        file(0),
        quiet,
        0,
    );
    let sharing = share_error(u16::from(SHARING_DENY_NONE), ShareReply::Retry);
    let retry = share_error(
        SF_IS_FCB | u16::from(SHARING_DENY_NONE),
        ShareReply::Retry,
    );
    let share_round = do_share_check(SF_IS_FCB, 3, 4, 1);
    let no_retry = do_share_check(SF_IS_FCB, 3, 4, 0);

    crate::bios::mscon::con_write_bytes(b"OPEN AC");
    put_hex8(status_byte(accepted));
    crate::bios::mscon::con_write_bytes(b" BAD");
    put_hex8(both_bad(bad_share, bad_access));
    crate::bios::mscon::con_write_bytes(b" NET");
    put_hex8(status_byte(net_fcb));
    crate::bios::mscon::con_write_bytes(b" NA");
    put_hex8(status_byte(net_access));
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(status_byte(plain_net));
    crate::bios::mscon::con_write_bytes(b" BLK");
    put_hex8(open_status(blocked));
    crate::bios::mscon::con_write_bytes(b" RO");
    put_hex8(open_status(readonly_write));
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(open_status(readonly_read));
    crate::bios::mscon::con_write_bytes(b" FCB");
    put_hex8(open_status(fcb_write));
    crate::bios::mscon::con_write_bytes(b" NFC");
    put_hex8(open_status(net_fcb_write));
    crate::bios::mscon::con_write_bytes(b" DIR");
    put_hex8(open_status(directory));
    crate::bios::mscon::con_write_bytes(b" VOL");
    put_hex8(open_status(volume));
    crate::bios::mscon::con_write_bytes(b" DEV");
    put_hex8(open_status(device));
    crate::bios::mscon::con_write_bytes(b" FO");
    put_hex8(fast_of(device));
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex8(open_status(not_found));
    crate::bios::mscon::con_write_bytes(b" NP");
    put_hex8(both_open(bad_path, empty_tail, ERROR_PATH_NOT_FOUND));
    crate::bios::mscon::con_write_bytes(b" V");
    put_hex8(verb_of(sequential, VERB_OPEN, false));
    crate::bios::mscon::con_write_bytes(b" V");
    put_hex8(verb_of(extended, VERB_EXTENDED, true));
    crate::bios::mscon::con_write_bytes(b" SHR");
    put_hex8(share_status(sharing));
    crate::bios::mscon::con_write_bytes(if retry.is_ok() { b" RT00" } else { b" RTFF" });
    crate::bios::mscon::con_write_bytes(if share_round.is_ok() && no_retry.is_err() {
        b" SHOK"
    } else {
        b" SHNO"
    });
    crate::bios::mscon::con_write_bytes(b" PID");
    put_hex16(recorded_process(fcb_write, readonly_read));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

struct LocalOpen {
    mode: u16,
    fast_flags: u8,
    error: Option<u16>,
}

fn remote(extended: bool) -> Opened {
    if extended {
        Opened::Remote {
            verb: VERB_EXTENDED,
            extended_cleared: true,
        }
    } else {
        Opened::Remote {
            verb: VERB_OPEN,
            extended_cleared: false,
        }
    }
}

fn finish_local(mode: u16, fast_flags: u8, lookup: Lookup) -> LocalOpen {
    let fast_flags = (fast_flags | FAST_OPEN_SET | SPECIAL_FILL_SET) & FAST_YES;
    match lookup {
        Lookup::Miss {
            final_name,
            final_len,
        } => LocalOpen {
            mode,
            fast_flags,
            error: Some(missing(final_name, final_len)),
        },
        Lookup::Hit {
            directory,
            device,
            attr,
        } => {
            if directory {
                return LocalOpen {
                    mode,
                    fast_flags,
                    error: Some(ERROR_ACCESS_DENIED),
                };
            }
            if device {
                return LocalOpen {
                    mode,
                    fast_flags,
                    error: None,
                };
            }
            if attr & ATTR_VOLUME != 0 {
                return LocalOpen {
                    mode,
                    fast_flags,
                    error: Some(ERROR_ACCESS_DENIED),
                };
            }
            if attr & ATTR_READ_ONLY != 0 {
                return match readonly_mode(mode) {
                    Ok(mode) => LocalOpen {
                        mode,
                        fast_flags,
                        error: None,
                    },
                    Err(error) => LocalOpen {
                        mode,
                        fast_flags,
                        error: Some(error),
                    },
                };
            }
            LocalOpen {
                mode,
                fast_flags,
                error: None,
            }
        }
    }
}

fn missing(final_name: bool, final_len: u8) -> u16 {
    if !final_name {
        return ERROR_PATH_NOT_FOUND;
    }
    if final_len == 0 {
        return ERROR_PATH_NOT_FOUND;
    }
    ERROR_FILE_NOT_FOUND
}

fn readonly_mode(mode: u16) -> Result<u16, u16> {
    if mode & SF_IS_FCB != 0 {
        return Ok(mode & !u16::from(ACCESS_MASK));
    }
    let sharing = (mode as u8) & SHARING_MASK;
    if sharing == SHARING_NET_FCB {
        return Ok(mode & !u16::from(ACCESS_MASK));
    }
    let access = (mode as u8) & ACCESS_MASK;
    if access == OPEN_FOR_READ {
        return Ok(mode);
    }
    Err(ERROR_ACCESS_DENIED)
}

fn share_round(tries: u16, failures: &mut u16) -> bool {
    let mut left = tries;
    loop {
        if *failures == 0 {
            return true;
        }
        *failures -= 1;
        if left == 0 {
            return false;
        }
        left -= 1;
        if left == 0 {
            return false;
        }
    }
}

fn request(mode: u8, server: bool, fcb: bool, extended: bool) -> Request {
    Request {
        mode,
        server,
        fcb,
        extended,
        process: 1,
    }
}

fn file(attr: u8) -> Lookup {
    Lookup::Hit {
        directory: false,
        device: false,
        attr,
    }
}

fn status_byte(result: Result<u8, u16>) -> u8 {
    match result {
        Ok(value) => value,
        Err(error) => error as u8,
    }
}

fn open_status(result: Result<Opened, u16>) -> u8 {
    match result {
        Ok(Opened::Local { mode, .. }) => mode as u8,
        Ok(Opened::Remote { verb, .. }) => verb,
        Err(error) => error as u8,
    }
}

fn both_bad(left: Result<u8, u16>, right: Result<u8, u16>) -> u8 {
    if left == Err(ERROR_INVALID_ACCESS) && right == Err(ERROR_INVALID_ACCESS) {
        ERROR_INVALID_ACCESS as u8
    } else {
        0xff
    }
}

fn both_open(left: Result<Opened, u16>, right: Result<Opened, u16>, error: u16) -> u8 {
    if left == Err(error) && right == Err(error) {
        error as u8
    } else {
        0xff
    }
}

fn fast_of(result: Result<Opened, u16>) -> u8 {
    match result {
        Ok(Opened::Local { fast_flags, .. }) => fast_flags,
        _ => 0xff,
    }
}

fn verb_of(result: Result<Opened, u16>, verb: u8, extended_cleared: bool) -> u8 {
    match result {
        Ok(Opened::Remote {
            verb: got,
            extended_cleared: cleared,
        }) if got == verb && cleared == extended_cleared => got,
        _ => 0xff,
    }
}

fn share_status(result: Result<(), u16>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) => error as u8,
    }
}

fn recorded_process(fcb: Result<Opened, u16>, plain: Result<Opened, u16>) -> u16 {
    match (fcb, plain) {
        (
            Ok(Opened::Local {
                process: Some(process),
                ..
            }),
            Ok(Opened::Local { process: None, .. }),
        ) => process,
        _ => 0xffff,
    }
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
