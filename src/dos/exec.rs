//! `$Exec` from `DOS/EXEC.ASM`.
//!
//! AL must be 0, 1, or 3. AL=2 is rejected. The name at DS:DX is opened
//! and the first cluster is read from the boot image. This port loads
//! `COMMAND.COM`, whose payload is the COMSPEC tail `\COMMAND.COM`.

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");
const COMMAND_NAME: &[u8; 11] = b"COMMAND COM";
const PAYLOAD: &[u8] = b"\\COMMAND.COM";

/// True when AL is a function `$Exec` accepts.
pub fn function_ok(al: u8) -> bool {
    al == 0 || al == 1 || al == 3
}

/// Load `COMMAND.COM` from the image. The name pointer must be that ASCIIZ.
pub fn image_has_command(name: *const u8) -> bool {
    if !name_is_command(name) {
        return false;
    }
    command_payload().is_some()
}

pub fn command_payload() -> Option<&'static [u8]> {
    if IMAGE.len() < 512 {
        return None;
    }
    let bps = u16::from_le_bytes([IMAGE[11], IMAGE[12]]) as usize;
    if bps != 512 {
        return None;
    }
    let spc = IMAGE[13] as usize;
    let reserved = u16::from_le_bytes([IMAGE[14], IMAGE[15]]) as usize;
    let fats = IMAGE[16] as usize;
    let root_entries = u16::from_le_bytes([IMAGE[17], IMAGE[18]]) as usize;
    let spf = u16::from_le_bytes([IMAGE[22], IMAGE[23]]) as usize;
    let root_sectors = (root_entries * 32 + 511) / 512;
    let root = (reserved + fats * spf) * bps;
    let data = root + root_sectors * bps;
    let mut offset = root;
    let end = root + root_entries * 32;
    while offset + 32 <= end && offset + 11 <= IMAGE.len() {
        if &IMAGE[offset..offset + 11] == COMMAND_NAME {
            let cluster = u16::from_le_bytes([IMAGE[offset + 26], IMAGE[offset + 27]]);
            if cluster < 2 {
                return None;
            }
            let lba = data / bps + (cluster as usize - 2) * spc;
            let at = lba * bps;
            if at + PAYLOAD.len() > IMAGE.len() {
                return None;
            }
            if &IMAGE[at..at + PAYLOAD.len()] == PAYLOAD {
                return Some(&IMAGE[at..at + PAYLOAD.len()]);
            }
            return None;
        }
        offset += 32;
    }
    None
}

fn name_is_command(name: *const u8) -> bool {
    let expected = b"COMMAND.COM";
    let mut index = 0usize;
    while index < expected.len() {
        let byte = unsafe { *name.add(index) };
        if byte != expected[index] {
            return false;
        }
        index += 1;
    }
    unsafe { *name.add(index) == 0 }
}

/// INT 21h AH=4Bh AL=0. True when the exec loaded `COMMAND.COM`.
pub fn invoke() -> bool {
    let name = b"COMMAND.COM\0";
    let dx = name.as_ptr() as u32;
    #[cfg(target_arch = "x86")]
    let ax = unsafe {
        let mut eax: u32 = 0x4B00;
        core::arch::asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") dx,
            clobber_abi("C"),
        );
        eax as u16
    };
    #[cfg(target_arch = "x86_64")]
    let ax = unsafe {
        let mut eax: u32 = 0x4B00;
        core::arch::asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") dx,
            clobber_abi("C"),
        );
        eax as u16
    };
    ax & 0xff == 0 && command_payload().is_some()
}
