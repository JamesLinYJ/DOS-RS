//! Boot-disk BIOS services. The fixed-disk path reads the primary ATA master.
//! The embedded floppy uses 18 sectors per track and two heads.

const ROMREAD: u8 = 2;
const FNON_REMOVABLE: u16 = 0x0001;
const FCHANGELINE: u16 = 0x0002;
const FI_AM_MULT: u16 = 0x0010;
const FI_OWN_PHYSICAL: u16 = 0x0020;
const FCHANGED_BY_FORMAT: u16 = 0x0100;
const UNFORMATTED_MEDIA: u16 = 0x0200;
const ROM_WRITE: u8 = 3;
const ROM_VERIFY: u8 = 4;
const SOFT_ECC: u8 = 0x11;
const WRITE_FAULT: u8 = 0xCC;
const ACCESS_MAX: u8 = 5;
/// `MSBDATA.INC` `Secrete_Code`. The word is the two characters `jk`.
const SECRETE_CODE: u16 = u16::from_le_bytes(*b"jk");
const FFHARDFILE: u8 = 5;
const ERROR_AH: u16 = 0x8100;

/// `MEDIA$CHK` for the fixed disk. `Secrete_Code` is `jk`, the disk is not
/// removable, and format has not changed it, so the result is 1 (no change).
pub fn media_check() -> i16 {
    if SECRETE_CODE != u16::from_le_bytes(*b"jk") {
        return 0;
    }
    let flags = FNON_REMOVABLE;
    let mut source: i16 = 1;
    if flags & FCHANGED_BY_FORMAT != 0 {
        if flags & FNON_REMOVABLE != 0 {
            source = -1;
        } else {
            source = 0;
        }
    } else if flags & FNON_REMOVABLE == 0 {
        source = 0;
    }
    source
}

/// `SETDRIVE`. One BDS, logical drive 0. Carry when the letter is absent.
pub fn set_drive(logical: u8) -> bool {
    logical == 0
}

/// `DSK$REM`. A fixed disk takes the busy exit.
pub fn dsk_removable() -> u16 {
    if FNON_REMOVABLE & FNON_REMOVABLE != 0 {
        0x0300
    } else {
        0x0100
    }
}

/// `CHECKSINGLE`. A fixed disk, or a drive that is not shared, returns at once.
pub fn check_single() -> bool {
    let flags = FNON_REMOVABLE;
    flags & (FNON_REMOVABLE | FI_OWN_PHYSICAL) != 0 || flags & FI_AM_MULT == 0
}

/// `CHECK_TIME_OF_ACCESS`. SI comes back 1 when little or no time has passed.
pub fn check_time_of_access(now_hi: u16, now_lo: u16, last_hi: u16, last_lo: u16, accesses: u8) -> (i16, u8) {
    let mut source: i16 = 1;
    let mut count = accesses;
    let lo_borrow = now_lo < last_lo;
    let elapsed_lo = now_lo.wrapping_sub(last_lo);
    let elapsed_hi = now_hi.wrapping_sub(last_hi).wrapping_sub(u16::from(lo_borrow));
    if elapsed_hi != 0 {
        source = 0;
    } else if elapsed_lo == 0 {
        count = count.saturating_add(1);
        if count >= ACCESS_MAX {
            count = ACCESS_MAX - 1;
            source = 0;
        }
    } else if elapsed_lo > 36 {
        source = 0;
    }
    (source, count)
}

/// `CHECK_WRAP` for a fixed disk. Sector numbers past the track spill onto
/// the next head, and past the last head onto the next cylinder.
pub fn check_wrap(packed: u16, data_high: u8, seclim: u16, hdlim: u16) -> (u16, u8) {
    if seclim == 0 || hdlim == 0 {
        return (packed, data_high);
    }
    let sector = packed & 0x3f;
    if sector <= seclim {
        return (packed, data_high);
    }
    let mut heads = u32::from(sector / seclim);
    let mut new_sector = sector % seclim;
    if new_sector == 0 {
        new_sector = seclim;
        heads = heads.saturating_sub(1);
    }
    let mut head = u32::from(data_high) + heads;
    let mut cyl = u32::from((packed >> 8) & 0xff) | u32::from((packed & 0xc0) << 2);
    if head > u32::from(hdlim) {
        let wraps = head / u32::from(hdlim);
        head %= u32::from(hdlim);
        if head == 0 {
            head = u32::from(hdlim);
            if wraps > 0 {
                cyl += wraps - 1;
            }
        } else {
            cyl += wraps;
        }
    }
    let data_high = (head as u8).wrapping_sub(1);
    let count_high = (cyl & 0xff) as u8;
    let count_low = ((new_sector as u8) & 0x3f) | (((cyl >> 2) as u8) & 0xc0);
    (u16::from(count_high) << 8 | u16::from(count_low), data_high)
}

/// `READBOOTSEC` sanity check. A short jump plus NOP, and a media byte in
/// the F0-FF range, keeps BX at 0.
/// `GET$BPB` for a fixed disk does not rebuild the BPB. The bytes-per-sector
/// field already in the boot sector is the value DOS is handed.
pub fn bpb_bytes_per_sector(sector: &[u8]) -> Option<u16> {
    if !boot_sector_valid(sector) || sector.len() < 13 {
        return None;
    }
    let bps = u16::from_le_bytes([sector[11], sector[12]]);
    if bps == 0 { None } else { Some(bps) }
}

/// `READFAT` / `READ_SECTOR` of the first FAT sector. The media byte is the
/// first byte of a valid FAT.
pub fn image_fat_media() -> Option<u8> {
    let image = msload_image();
    if image.len() < 513 {
        return None;
    }
    fat_media(&image[512..])
}

pub fn fat_media(fat: &[u8]) -> Option<u8> {
    let byte = *fat.first()?;
    if byte & 0xF0 == 0xF0 { Some(byte) } else { None }
}

/// `MOVE`. Copy one sector, 256 words, the way the disk buffer mover does.
pub fn move_sector(src: &[u8], dst: &mut [u8]) -> bool {
    if src.len() < 512 || dst.len() < 512 {
        return false;
    }
    dst[..512].copy_from_slice(&src[..512]);
    true
}

/// `DSK$READ` bounds. An unformatted disk or a sector past the limit is an error.
pub fn read_in_range(flags: u16, sector: u32, count: u16, drvlim: u16) -> bool {
    if flags & UNFORMATTED_MEDIA != 0 || count == 0 {
        return false;
    }
    if drvlim == 0 {
        return true;
    }
    sector.saturating_add(u32::from(count)) <= u32::from(drvlim)
}

pub fn boot_sector_valid(sector: &[u8]) -> bool {
    if sector.len() < 22 {
        return false;
    }
    let jump_ok = sector[0] == 0x69
        || sector[0] == 0xE9
        || (sector[0] == 0xEB && sector[2] == 0x90);
    if !jump_ok {
        return false;
    }
    sector[21] & 0xF0 == 0xF0
}

/// `IOSETUP` records the physical drive. The disk-base table itself is not
/// present under `-kernel`, so the settle bytes stay at the fast value 1.
pub fn io_setup(drivenum: u8) -> u8 {
    let _fast_settle = 1u8;
    drivenum
}

/// `SET_TIM` stores the tick words when they differ from the last access.
pub fn set_tim(now_hi: u16, now_lo: u16, last_hi: u16, last_lo: u16) -> (u16, u16, bool) {
    if now_lo == last_lo && now_hi == last_hi {
        (last_hi, last_lo, false)
    } else {
        (now_hi, now_lo, true)
    }
}

/// `INT_2F_13`. AH=13h replaces the saved INT 13h vector and returns the old one.
pub fn int2f_13(function: u8, old: u32, new_handler: u32) -> Option<u32> {
    if function == 0x13 {
        let _installed = new_handler;
        Some(old)
    } else {
        None
    }
}

/// Reads use the fast settle. A fixed disk always does. A reset (`step_drv`
/// of -1) forces the slow settle on a write.
pub fn fast_settle(function: u8, formfactor: u8, step_drv: i8) -> bool {
    if formfactor == FFHARDFILE {
        return true;
    }
    if step_drv == -1 {
        return false;
    }
    function == ROMREAD || function == ROM_VERIFY
}

/// Soft ECC on a verify is ignored once the retry count is exhausted.
pub fn verify_soft_ecc(function: u8, retries: u16) -> bool {
    function == SOFT_ECC && retries == 0
}

/// The fixed-disk paths that do not issue INT 13h. False means one of them
/// disagreed with the BDS.
pub fn fixed_disk_paths() -> bool {
    let (media, _) = check_time_of_access(0, 10, 0, 0, 0);
    let (wrapped, _) = check_wrap(18, 0, 17, 2);
    let (ticks_hi, ticks_lo, stored) = set_tim(0, 1, 0, 0);
    set_drive(0)
        && !set_drive(1)
        && dsk_removable() == 0x0300
        && check_single()
        && io_setup(0x80) == 0x80
        && media == 1
        && wrapped & 0x3f == 1
        && stored
        && ticks_hi == 0
        && ticks_lo == 1
        && fast_settle(ROMREAD, FFHARDFILE, 0)
        && !fast_settle(ROM_WRITE, 0, -1)
        && verify_soft_ecc(SOFT_ECC, 0)
        && !verify_soft_ecc(WRITE_FAULT, 1)
        && int2f_13(0x13, 0x1000, 0x2000) == Some(0x1000)
        && int2f_13(0, 0x1000, 0x2000).is_none()
        && !ask_rom_media(FNON_REMOVABLE)
        && dma_fit(0x1000, 512) == 512
        && dma_fit(0xff00, 512) == 0x100
        && read_in_range(FNON_REMOVABLE, 0, 1, 10000)
        && !read_in_range(UNFORMATTED_MEDIA, 0, 1, 10000)
        && move_sector(&[0u8; 512], &mut [0u8; 512])
}

/// `BLOCK13` refuses a transfer that crosses a 64KiB physical boundary.
/// The return value is how many bytes fit before that boundary.
pub fn dma_fit(addr: u32, bytes: u32) -> u32 {
    if bytes == 0 {
        return 0;
    }
    let end = addr.wrapping_add(bytes);
    let crosses = (addr ^ end.wrapping_sub(1)) & 0xffff_0000 != 0;
    if !crosses {
        bytes
    } else {
        (addr & 0xffff_0000).wrapping_add(0x1_0000).wrapping_sub(addr)
    }
}

fn ask_rom_media(flags: u16) -> bool {
    flags & FCHANGELINE != 0
}

/// Read one sector at sector 0 of logical drive 0. Returns the status word.
pub fn read_sector0(buffer: &mut [u8]) -> u16 {
    let Some(sector) = buffer.get_mut(..512).and_then(|s| s.try_into().ok()) else {
        return ERROR_AH | 8;
    };
    if super::ata::capacity().is_some() && super::ata::read(0, sector) {
        0x0100
    } else {
        ERROR_AH | 2
    }
}

/// Read cylinder 0, head 0, sector 1 of drive 0. That is logical sector 0
/// of the floppy image. The hard-disk BDS is left alone.
pub fn read_floppy_boot(buffer: &mut [u8; 512]) -> bool {
    let carry = read_chs(ROMREAD, 1, 1, 0, 0, 0, buffer);
    !carry && buffer[510] == 0x55 && buffer[511] == 0xAA
}

pub(crate) fn read_chs(
    ah_cmd: u8,
    count: u8,
    sector: u8,
    cyl: u16,
    head: u8,
    drive: u8,
    buf: &mut [u8],
) -> bool {
    if ah_cmd == 0 {
        return drive != 0;
    }
    if drive != 0 || ah_cmd != ROMREAD || count == 0 || !(1..=18).contains(&sector) || head >= 2 {
        return true;
    }
    let lba = (usize::from(cyl) * 2 + usize::from(head)) * 18 + usize::from(sector - 1);
    let bytes = usize::from(count) * 512;
    let start = lba * 512;
    let Some(source) = msload_image().get(start..start + bytes) else {
        return true;
    };
    let Some(target) = buf.get_mut(..bytes) else {
        return true;
    };
    target.copy_from_slice(source);
    false
}

fn msload_image() -> &'static [u8] {
    include_bytes!("../../disks/msload.img")
}
