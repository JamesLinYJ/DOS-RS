//! Directory nodes from `DOS/MKNODE.ASM`.
//!
//! A free directory slot is reused. The root cannot grow. `.` and `..` are
//! eleven-byte names with the directory attribute. An existing directory is
//! always an error. A volume or directory create cannot reuse a name. Devices
//! skip the read-only test. A missing name with metas is a bad path. Extended
//! open fails with code 7 when the high nibble of the action is zero. Clusters
//! below 2 or above the disk maximum are left alone. A virtual open is closed
//! once.

const VOLUME: u8 = 0x08;
const DIRECTORY: u8 = 0x10;
const CLEAN: u8 = 0x40;

pub enum DirGrow {
    Free(u16),
    Grow,
}

pub enum Lookup {
    Directory,
    File { read_only: bool, matches: bool },
    Device,
    Absent { plain: bool },
    BadPath,
}

/// `BUILDDIR`. A recorded free entry is returned. Otherwise only a non-root
/// directory can grow.
pub fn build_dir(free_entry: u16, dir_start: u16) -> Result<DirGrow, ()> {
    if free_entry != 0xffff {
        return Ok(DirGrow::Free(free_entry));
    }
    if dir_start == 0 {
        return Err(());
    }
    Ok(DirGrow::Grow)
}

/// `SETDOTENT`. The name is `.` or `..` padded with spaces.
pub fn dot_name(parent: bool) -> [u8; 11] {
    let mut name = [b' '; 11];
    name[0] = b'.';
    if parent {
        name[1] = b'.';
    }
    name
}

/// `MakeNode`.
pub fn make_node(
    attribute: u8,
    truncate_ok: bool,
    extended: bool,
    create_if_missing: bool,
    share_ok: bool,
    found: Lookup,
) -> Result<u8, u8> {
    match found {
        Lookup::BadPath => Err(4),
        Lookup::Absent { plain } => {
            if !plain {
                return Err(4);
            }
            if extended && !create_if_missing {
                return Err(7);
            }
            if attribute & DIRECTORY != 0 {
                return Ok(0);
            }
            if !share_ok {
                return Err(6);
            }
            Ok(0)
        }
        Lookup::Directory => Err(1),
        Lookup::Device | Lookup::File { .. } => {
            if attribute & (VOLUME | DIRECTORY) != 0 {
                return Err(5);
            }
            if let Lookup::Device = found {
                if !share_ok {
                    return Err(6);
                }
                return Ok(3);
            }
            if !truncate_ok {
                return Err(3);
            }
            if let Lookup::File { read_only, matches } = found {
                if read_only {
                    return Err(5);
                }
                if !matches {
                    return Err(5);
                }
            }
            if !share_ok {
                return Err(6);
            }
            Ok(0)
        }
    }
}

/// `NEWENTRY` volume check. A second volume label is rejected.
pub fn second_volume(making_volume: bool, label_found: bool) -> bool {
    if !making_volume {
        return false;
    }
    label_found
}

/// `FREEENT`. Clusters below 2 or past the disk maximum are not released.
pub fn release_chain(cluster: u16, max_cluster: u16) -> bool {
    if cluster < 2 {
        return false;
    }
    if cluster > max_cluster {
        return false;
    }
    true
}

/// `CHECK_VIRT_OPEN`. A set flag is cleared and the open is closed.
pub fn close_virtual(open: u8) -> bool {
    open != 0
}

/// `DOOPEN` attribute and flags. Devices have attribute 0 and size 0.
pub fn fill_sft(device: bool, device_id: u8, file_attr: u8) -> (u8, u8) {
    let mut attribute = 0;
    if !device {
        attribute = file_attr;
    }
    (attribute, device_id | CLEAN)
}

pub fn show() {
    let free = build_dir(5, 0);
    let root = build_dir(0xffff, 0);
    let grow = build_dir(0xffff, 2);
    let dot = dot_name(false);
    let parent = dot_name(true);
    let exists_dir = make_node(0, true, false, true, true, Lookup::Directory);
    let make_dir = make_node(
        DIRECTORY,
        true,
        false,
        true,
        true,
        Lookup::File {
            read_only: false,
            matches: true,
        },
    );
    let no_trunc = make_node(
        0,
        false,
        false,
        true,
        true,
        Lookup::File {
            read_only: false,
            matches: true,
        },
    );
    let read_only = make_node(
        0,
        true,
        false,
        true,
        true,
        Lookup::File {
            read_only: true,
            matches: true,
        },
    );
    let device = make_node(0, false, false, true, true, Lookup::Device);
    let bad_path = make_node(0, true, false, true, true, Lookup::BadPath);
    let metas = make_node(
        0,
        true,
        false,
        true,
        true,
        Lookup::Absent { plain: false },
    );
    let extended = make_node(
        0,
        true,
        true,
        false,
        true,
        Lookup::Absent { plain: true },
    );
    let created = make_node(
        0,
        true,
        false,
        true,
        true,
        Lookup::Absent { plain: true },
    );
    let share = make_node(
        0,
        true,
        false,
        true,
        false,
        Lookup::Absent { plain: true },
    );
    let directory = make_node(
        DIRECTORY,
        true,
        false,
        true,
        false,
        Lookup::Absent { plain: true },
    );
    let (device_attr, device_flags) = fill_sft(true, 0x80, 0x20);
    let (file_attr, file_flags) = fill_sft(false, 0x02, 0x20);
    crate::bios::mscon::con_write_bytes(b"MKNODE BD");
    match free {
        Ok(DirGrow::Free(entry)) => put_hex16(entry),
        _ => put_hex16(0xffff),
    }
    crate::bios::mscon::con_write_bytes(if root.is_err() { b" RT" } else { b" OK" });
    crate::bios::mscon::con_write_bytes(match grow {
        Ok(DirGrow::Grow) => b" GR",
        _ => b" NG",
    });
    crate::bios::mscon::con_write_bytes(b" DOT");
    put_hex8(dot[0]);
    put_hex8(dot[1]);
    crate::bios::mscon::con_write_bytes(b" DD");
    put_hex8(parent[0]);
    put_hex8(parent[1]);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(node_code(exists_dir));
    crate::bios::mscon::con_write_bytes(b" MD");
    put_hex8(node_code(make_dir));
    crate::bios::mscon::con_write_bytes(b" NT");
    put_hex8(node_code(no_trunc));
    crate::bios::mscon::con_write_bytes(b" RO");
    put_hex8(node_code(read_only));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(node_code(device));
    crate::bios::mscon::con_write_bytes(b" BP");
    put_hex8(node_code(bad_path));
    crate::bios::mscon::con_write_bytes(b" MT");
    put_hex8(node_code(metas));
    crate::bios::mscon::con_write_bytes(b" EO");
    put_hex8(node_code(extended));
    crate::bios::mscon::con_write_bytes(b" NW");
    put_hex8(node_code(created));
    crate::bios::mscon::con_write_bytes(b" SH");
    put_hex8(node_code(share));
    crate::bios::mscon::con_write_bytes(b" DIR");
    put_hex8(node_code(directory));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(if release_chain(0, 10) { 1 } else { 0 });
    put_hex8(if release_chain(1, 10) { 1 } else { 0 });
    put_hex8(if release_chain(5, 10) { 1 } else { 0 });
    put_hex8(if release_chain(20, 10) { 1 } else { 0 });
    crate::bios::mscon::con_write_bytes(if close_virtual(0) { b" VC01" } else { b" VC00" });
    crate::bios::mscon::con_write_bytes(if close_virtual(1) { b" VC01" } else { b" VC00" });
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(device_attr);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex8(device_flags);
    crate::bios::mscon::con_write_bytes(b" FA");
    put_hex8(file_attr);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex8(file_flags);
    crate::bios::mscon::con_write_bytes(if second_volume(true, true) {
        b" VOL1"
    } else {
        b" VOL0"
    });
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn node_code(result: Result<u8, u8>) -> u8 {
    match result {
        Ok(kind) => kind,
        Err(error) => error,
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
