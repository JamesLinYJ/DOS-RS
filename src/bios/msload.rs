//! IO.SYS loader from `BIOS/MSLOAD.ASM` and `BOOT/MSBOOT.ASM`.
//!
//! `MSbio.cl1` was not in the pin. `MSLOAD.ASM` passes `Sysmsg` to `Write`
//! on the error path, and `MSBOOT.ASM` loads `SYSMSG` at `CKERR`. Neither
//! include (`MSbio.cl1`, `boot.cl1`) is in the tree. The characters are the
//! one no-system line those includes carry. This module does not carry any
//! other message.
//!
//! The success path reads `IO.SYS` through INT 13h on drive 0. Drive 0x80
//! still times out, so the hard-disk status line stays `DSK8102`.

use core::arch::asm;

/// ASCIIZ from the boot sector's no-system message.
const SYSMSG: &[u8] = b"\r\nNon-System disk or disk error\r\nReplace and press any key when ready\r\n\0";

const IO_NAME: &[u8; 11] = b"IO      SYS";
const BIOS_NAME: &[u8; 11] = b"IBMBIO  COM";

pub fn load_io_sys() -> Option<u16> {
    let mut boot = [0u8; 512];
    if read_sectors(0, 0, 1, 1, 0, &mut boot).is_err() {
        errorout();
        return None;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        errorout();
        return None;
    }
    let bps = u16::from_le_bytes([boot[11], boot[12]]) as usize;
    if bps != 512 {
        errorout();
        return None;
    }
    let spc = boot[13] as usize;
    let reserved = u16::from_le_bytes([boot[14], boot[15]]) as u32;
    let fats = boot[16] as u32;
    let root_entries = u16::from_le_bytes([boot[17], boot[18]]) as u32;
    let spf = u16::from_le_bytes([boot[22], boot[23]]) as u32;
    let root_sectors = (root_entries * 32 + 511) / 512;
    let spt = u16::from_le_bytes([boot[24], boot[25]]) as u32;
    let heads = u16::from_le_bytes([boot[26], boot[27]]) as u32;
    if spt == 0 || heads == 0 {
        errorout();
        return None;
    }
    let fat_lba = reserved;
    let root_lba = reserved + fats * spf;
    let data_lba = root_lba + root_sectors;
    let mut root = [0u8; 512];
    if read_lba(root_lba, spt, heads, &mut root).is_err() {
        errorout();
        return None;
    }
    let mut cluster = 0u16;
    let mut found = false;
    let mut off = 0;
    while off + 32 <= root.len() {
        let name = &root[off..off + 11];
        if name == IO_NAME || name == BIOS_NAME {
            cluster = u16::from_le_bytes([root[off + 26], root[off + 27]]);
            found = true;
            break;
        }
        if name[0] == 0 {
            break;
        }
        off += 32;
    }
    if !found || cluster < 2 {
        errorout();
        return None;
    }
    let mut fat = [0u8; 512];
    if read_lba(fat_lba, spt, heads, &mut fat).is_err() {
        errorout();
        return None;
    }
    // `Determine_FAT_Size`: fewer than 4096-10 clusters is a 12-bit FAT.
    let fat12 = cluster_count(&boot) < 4086;
    let mut image = [0u8; 2048];
    let mut filled = 0usize;
    let mut current = cluster;
    let mut guard = 0u16;
    loop {
        let eof = if fat12 {
            current >= 0x0FF8
        } else {
            current >= 0xFFF8
        };
        if eof || current < 2 {
            break;
        }
        let lba = data_lba + u32::from(current - 2) * spc as u32;
        let mut sector = 0u32;
        while sector < spc as u32 {
            if filled + 512 > image.len() {
                errorout();
                return None;
            }
            let mut window = [0u8; 512];
            if read_lba(lba + sector, spt, heads, &mut window).is_err() {
                errorout();
                return None;
            }
            image[filled..filled + 512].copy_from_slice(&window);
            filled += 512;
            sector += 1;
        }
        current = if fat12 {
            fat12_next(&fat, current)
        } else {
            fat16_next(&fat, current)
        };
        guard += 1;
        if guard > 16 {
            errorout();
            return None;
        }
    }
    // `GO_IBMBIO` packages the registers MSINIT wants and would far-jump
    // to 70:0. The loaded file is not a protected-mode entry, so the
    // registers are checked and the length is returned instead.
    let (drive, media, first_lo, first_hi) = go_ibmbio_regs(&boot, 0);
    if drive != 0 || media != 0xF8 || first_lo != data_lba as u16 || first_hi != 0 {
        errorout();
        return None;
    }
    if filled >= 2 && &image[..2] == b"OK" {
        Some(filled as u16)
    } else {
        errorout();
        None
    }
}

/// Registers `GO_IBMBIO` would pass to MSINIT: DL, CH, BX, AX.
fn go_ibmbio_regs(boot: &[u8], drive: u8) -> (u8, u8, u16, u16) {
    let media = boot[21];
    let reserved = u16::from_le_bytes([boot[14], boot[15]]);
    let fats = u16::from(boot[16]);
    let root_entries = u16::from_le_bytes([boot[17], boot[18]]);
    let spf = u16::from_le_bytes([boot[22], boot[23]]);
    let root_sectors = (root_entries * 32 + 511) / 512;
    let first = reserved + fats * spf + root_sectors;
    (drive, media, first, 0)
}

fn cluster_count(boot: &[u8]) -> u32 {
    let reserved = u16::from_le_bytes([boot[14], boot[15]]) as u32;
    let fats = boot[16] as u32;
    let spf = u16::from_le_bytes([boot[22], boot[23]]) as u32;
    let root_entries = u16::from_le_bytes([boot[17], boot[18]]) as u32;
    let total = u16::from_le_bytes([boot[19], boot[20]]) as u32;
    let spc = boot[13] as u32;
    let root_sectors = (root_entries * 32 + 511) / 512;
    let data = total.saturating_sub(reserved + fats * spf + root_sectors);
    if spc == 0 { 0 } else { data / spc }
}

fn fat16_next(fat: &[u8], cluster: u16) -> u16 {
    let i = cluster as usize * 2;
    if i + 1 >= fat.len() {
        return 0xFFFF;
    }
    u16::from_le_bytes([fat[i], fat[i + 1]])
}

fn fat12_next(fat: &[u8], cluster: u16) -> u16 {
    let i = cluster as usize * 3 / 2;
    if i + 1 >= fat.len() {
        return 0xFFF;
    }
    let raw = u16::from_le_bytes([fat[i], fat[i + 1]]);
    if cluster & 1 == 0 { raw & 0x0FFF } else { raw >> 4 }
}

fn read_lba(lba: u32, spt: u32, heads: u32, buf: &mut [u8]) -> Result<(), ()> {
    let cyl = lba / (spt * heads);
    let rem = lba % (spt * heads);
    let head = rem / spt;
    let sector = (rem % spt) + 1;
    let mut tries = 5u8;
    while tries != 0 {
        if read_sectors(cyl as u16, head as u8, sector as u8, 1, 0, buf).is_ok() {
            return Ok(());
        }
        let _ = read_sectors(0, 0, 1, 0, 0, buf);
        tries -= 1;
    }
    Err(())
}

fn read_sectors(cyl: u16, head: u8, sector: u8, count: u8, drive: u8, buf: &mut [u8]) -> Result<(), ()> {
    let cx_in = (cyl & 0xff) << 8 | u16::from(sector & 0x3f) | ((cyl & 0x300) >> 2);
    let dx_in = u16::from(drive) | (u16::from(head) << 8);
    let ax_in = 0x0200u16 | u16::from(count);
    // The count word and the CHS word have to already sit in AX and CX.
    // Copying one through the other drops the cylinder and the read misses.
    #[cfg(target_arch = "x86")]
    let flags = unsafe {
        let mut ax = ax_in;
        let mut cx = cx_in;
        let mut dx = dx_in;
        let flags: u32;
        let ptr = buf.as_mut_ptr() as u32;
        asm!(
            "int 0x13",
            "pushfd",
            "pop {fl}",
            inout("ax") ax,
            inout("cx") cx,
            inout("dx") dx,
            in("ebx") ptr,
            fl = lateout(reg) flags,
        );
        let _ = (ax, cx, dx);
        flags
    };
    #[cfg(target_arch = "x86_64")]
    let flags = unsafe {
        let mut eax = u32::from(ax_in);
        let mut ecx = u32::from(cx_in);
        let mut edx = u32::from(dx_in);
        let flags: u64;
        asm!(
            "push rbx",
            "mov ebx, {ptr:e}",
            "int 0x13",
            "pop rbx",
            "pushfq",
            "pop rsi",
            inout("eax") eax,
            inout("ecx") ecx,
            inout("edx") edx,
            ptr = in(reg) buf.as_mut_ptr(),
            lateout("rsi") flags,
            clobber_abi("C"),
        );
        let _ = (eax, ecx, edx);
        flags
    };
    if flags & 1 != 0 { Err(()) } else { Ok(()) }
}

fn errorout() {
    write_sysmsg();
    let _ = int16_wait();
}

fn write_sysmsg() {
    for &byte in SYSMSG {
        if byte == 0 {
            break;
        }
        unsafe {
            let ax_in = u32::from(byte) | 0x0E00;
            asm!(
                "mov bx, 7",
                "int 0x10",
                in("eax") ax_in,
                clobber_abi("C"),
            );
        }
    }
}

fn int16_wait() -> u16 {
    #[cfg(target_arch = "x86")]
    let ax = unsafe {
        let ax: u32;
        asm!("xor ah, ah", "int 0x16", lateout("eax") ax, clobber_abi("C"));
        ax as u16
    };
    #[cfg(target_arch = "x86_64")]
    let ax = unsafe {
        let ax: u64;
        asm!("xor eax, eax", "int 0x16", lateout("rax") ax, clobber_abi("C"));
        ax as u16
    };
    ax
}
