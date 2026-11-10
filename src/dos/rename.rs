//! `DOS_RENAME` and `NEW_RENAME` from `DOS/RENAME.ASM`.
//!
//! A network rename issues redirector verb 17. Local drives are compared
//! after forcing bit 5, so `A` and `a` match. `error_no_more_files` from the
//! source search becomes "file not found"; any other search error is kept.
//! A directory path of 67 or more bytes is denied. A missing destination is
//! acceptable when the failure is the final name. `?` bytes are replaced
//! from the source name. A cross-directory rename of a directory is denied,
//! and a failed re-find of the source still returns success.

const ERROR_FILE_NOT_FOUND: u16 = 2;
const ERROR_PATH_NOT_FOUND: u16 = 3;
const ERROR_ACCESS_DENIED: u16 = 5;
const ERROR_NOT_SAME_DEVICE: u16 = 17;
const ERROR_NO_MORE_FILES: u16 = 18;
const ERROR_SHARING_VIOLATION: u16 = 32;
const ATTR_DIRECTORY: u8 = 0x10;
const VERB_RENAME: u8 = 17;
const FREED: u8 = 0xe5;
const PATH_LIMIT: u8 = 67;

#[derive(Clone, Copy)]
pub enum Route {
    Network,
    Local(Local),
}

#[derive(Clone, Copy)]
pub struct Local {
    pub source_drive: u8,
    pub dest_drive: u8,
    pub search: Search,
    pub dest: Dest,
    pub raw: [u8; 11],
    pub built_device: bool,
    pub dir_search_ok: bool,
    pub dest_exists: bool,
    pub find_failed: bool,
    pub dest_dir: u16,
    pub made: bool,
    pub source_found: bool,
}

#[derive(Clone, Copy)]
pub enum Search {
    Error(u16),
    Hit {
        directory: bool,
        path_len: u8,
        device: bool,
        share_denied: bool,
        source_dir: u16,
        attr: u8,
        source_name: [u8; 11],
    },
}

#[derive(Clone, Copy)]
pub enum Dest {
    Hit { device: bool },
    Miss {
        final_name: bool,
        final_len: u8,
        failed: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Renamed {
    Remote { verb: u8 },
    Done {
        name: [u8; 11],
        simple: bool,
        freed: u8,
        written: bool,
    },
}

/// Replace `?` in `raw` with the byte at the same index of `source`.
pub fn new_rename(raw: &[u8; 11], source: &[u8; 11]) -> [u8; 11] {
    let mut name = [0u8; 11];
    let mut index = 0usize;
    while index < name.len() {
        let byte = raw[index];
        name[index] = if byte == b'?' { source[index] } else { byte };
        index += 1;
    }
    name
}

/// `DOS_RENAME`.
pub fn dos_rename(route: Route) -> Result<Renamed, u16> {
    let local = match route {
        Route::Network => {
            return Ok(Renamed::Remote { verb: VERB_RENAME });
        }
        Route::Local(local) => local,
    };
    let source_drive = local.source_drive | 0x20;
    let dest_drive = local.dest_drive | 0x20;
    if source_drive != dest_drive {
        return Err(ERROR_NOT_SAME_DEVICE);
    }
    let hit = match local.search {
        Search::Error(code) => {
            if code == ERROR_NO_MORE_FILES {
                return Err(ERROR_FILE_NOT_FOUND);
            }
            return Err(code);
        }
        Search::Hit {
            directory,
            path_len,
            device,
            share_denied,
            source_dir,
            attr,
            source_name,
        } => {
            if directory && path_len >= PATH_LIMIT {
                return Err(ERROR_ACCESS_DENIED);
            }
            if device {
                return Err(ERROR_ACCESS_DENIED);
            }
            if share_denied {
                return Err(ERROR_SHARING_VIOLATION);
            }
            (source_dir, attr, source_name)
        }
    };
    let (source_dir, attr, source_name) = hit;
    match local.dest {
        Dest::Hit { device } => {
            if device {
                return Err(ERROR_ACCESS_DENIED);
            }
        }
        Dest::Miss {
            final_name,
            final_len,
            failed,
        } => {
            if !final_name {
                return Err(ERROR_PATH_NOT_FOUND);
            }
            if failed {
                return Err(ERROR_PATH_NOT_FOUND);
            }
            if final_len == 0 {
                return Err(ERROR_PATH_NOT_FOUND);
            }
        }
    }
    let name = new_rename(&local.raw, &source_name);
    if local.built_device {
        return Err(ERROR_ACCESS_DENIED);
    }
    if !local.dir_search_ok {
        return Err(ERROR_ACCESS_DENIED);
    }
    if local.dest_exists {
        return Err(ERROR_ACCESS_DENIED);
    }
    if local.find_failed {
        return Err(ERROR_ACCESS_DENIED);
    }
    if local.dest_dir == source_dir {
        if !local.source_found {
            return Ok(Renamed::Done {
                name,
                simple: true,
                freed: 0,
                written: false,
            });
        }
        return Ok(Renamed::Done {
            name,
            simple: true,
            freed: 0,
            written: true,
        });
    }
    if attr & ATTR_DIRECTORY != 0 {
        return Err(ERROR_ACCESS_DENIED);
    }
    if !local.made {
        return Err(ERROR_ACCESS_DENIED);
    }
    if !local.source_found {
        return Ok(Renamed::Done {
            name,
            simple: false,
            freed: 0,
            written: false,
        });
    }
    Ok(Renamed::Done {
        name,
        simple: false,
        freed: FREED,
        written: true,
    })
}

pub fn show() {
    let net = dos_rename(Route::Network);
    let drives = dos_rename(Route::Local(local(b'A', b'B', Search::Error(ERROR_NO_MORE_FILES))));
    let missing = dos_rename(Route::Local(local(b'A', b'a', Search::Error(ERROR_NO_MORE_FILES))));
    let kept = dos_rename(Route::Local(local(b'A', b'a', Search::Error(ERROR_PATH_NOT_FOUND))));
    let long = dos_rename(Route::Local(local(
        b'A',
        b'a',
        found(true, 67, false, false, 1, ATTR_DIRECTORY),
    )));
    let device = dos_rename(Route::Local(local(
        b'A',
        b'a',
        found(false, 0, true, false, 1, 0),
    )));
    let share = dos_rename(Route::Local(local(
        b'A',
        b'a',
        found(false, 0, false, true, 1, 0),
    )));
    let mut bad_path = ready(found(false, 0, false, false, 1, 0));
    bad_path.dest = Dest::Miss {
        final_name: false,
        final_len: 1,
        failed: false,
    };
    let mut failed = ready(found(false, 0, false, false, 1, 0));
    failed.dest = Dest::Miss {
        final_name: true,
        final_len: 1,
        failed: true,
    };
    let mut empty_tail = ready(found(false, 0, false, false, 1, 0));
    empty_tail.dest = Dest::Miss {
        final_name: true,
        final_len: 0,
        failed: false,
    };
    let mut dest_device = ready(found(false, 0, false, false, 1, 0));
    dest_device.dest = Dest::Hit { device: true };
    let mut built = ready(found(false, 0, false, false, 1, 0));
    built.built_device = true;
    let mut exists = ready(found(false, 0, false, false, 1, 0));
    exists.dest_exists = true;
    let cross_dir = dos_rename(Route::Local(ready(found(
        true,
        10,
        false,
        false,
        1,
        ATTR_DIRECTORY,
    ))));
    let mut stopped = ready(found(true, 66, false, false, 1, ATTR_DIRECTORY));
    stopped.dest_dir = 1;
    stopped.source_found = false;
    let simple = dos_rename(Route::Local(stopped_ok()));
    let moved = dos_rename(Route::Local(ready(found(false, 0, false, false, 1, 0))));
    crate::bios::mscon::con_write_bytes(b"RENAME NET");
    put_hex8(verb_of(net));
    crate::bios::mscon::con_write_bytes(b" DRV");
    put_hex8(status_of(drives));
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex8(status_of(missing));
    crate::bios::mscon::con_write_bytes(b" KEEP");
    put_hex8(status_of(kept));
    crate::bios::mscon::con_write_bytes(b" DL");
    put_hex8(status_of(long));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(status_of(device));
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex8(status_of(share));
    crate::bios::mscon::con_write_bytes(b" DP");
    put_hex8(status_of(dos_rename(Route::Local(bad_path))));
    crate::bios::mscon::con_write_bytes(b" DF");
    put_hex8(status_of(dos_rename(Route::Local(failed))));
    crate::bios::mscon::con_write_bytes(b" DZ");
    put_hex8(status_of(dos_rename(Route::Local(empty_tail))));
    crate::bios::mscon::con_write_bytes(b" DD");
    put_hex8(status_of(dos_rename(Route::Local(dest_device))));
    crate::bios::mscon::con_write_bytes(b" BD");
    put_hex8(status_of(dos_rename(Route::Local(built))));
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(status_of(dos_rename(Route::Local(exists))));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(status_of(cross_dir));
    crate::bios::mscon::con_write_bytes(b" ");
    put_stopped(dos_rename(Route::Local(stopped)));
    crate::bios::mscon::con_write_bytes(b" RN");
    put_done(simple);
    crate::bios::mscon::con_write_bytes(b" MV");
    put_hex8(freed_of(moved));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn stopped_ok() -> Local {
    let mut file = ready(found(true, 66, false, false, 1, ATTR_DIRECTORY));
    file.dest_dir = 1;
    file.source_found = true;
    file
}

fn found(
    directory: bool,
    path_len: u8,
    device: bool,
    share_denied: bool,
    source_dir: u16,
    attr: u8,
) -> Search {
    Search::Hit {
        directory,
        path_len,
        device,
        share_denied,
        source_dir,
        attr,
        source_name: *b"FILE    TXT",
    }
}

fn local(source_drive: u8, dest_drive: u8, search: Search) -> Local {
    Local {
        source_drive,
        dest_drive,
        search,
        dest: Dest::Miss {
            final_name: true,
            final_len: 1,
            failed: false,
        },
        raw: *b"?ILE????DOC",
        built_device: false,
        dir_search_ok: true,
        dest_exists: false,
        find_failed: false,
        dest_dir: 2,
        made: true,
        source_found: true,
    }
}

fn ready(search: Search) -> Local {
    local(b'A', b'a', search)
}

fn status_of(result: Result<Renamed, u16>) -> u8 {
    match result {
        Ok(_) => 0,
        Err(error) => error as u8,
    }
}

fn verb_of(result: Result<Renamed, u16>) -> u8 {
    match result {
        Ok(Renamed::Remote { verb }) => verb,
        _ => 0xff,
    }
}

fn freed_of(result: Result<Renamed, u16>) -> u8 {
    match result {
        Ok(Renamed::Done {
            simple: false,
            freed,
            written: true,
            ..
        }) => freed,
        _ => 0xff,
    }
}

fn put_stopped(result: Result<Renamed, u16>) {
    let text: &[u8] = match result {
        Ok(Renamed::Done { written: false, .. }) => b"SF00",
        _ => b"SFFF",
    };
    crate::bios::mscon::con_write_bytes(text);
}

fn put_done(result: Result<Renamed, u16>) {
    match result {
        Ok(Renamed::Done {
            name,
            simple: true,
            written: true,
            ..
        }) => crate::bios::mscon::con_write_bytes(&name),
        _ => crate::bios::mscon::con_write_bytes(b"???????????"),
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
