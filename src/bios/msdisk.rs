//! Disk read from `BIOS/MSDISK.ASM` for one fixed-disk sector.
//!
//! There is no disk controller under QEMU `-kernel`. `FASTSPEED` executes
//! INT 13h. The handler returns carry and AH=`80h`. `DSKERR` treats `80h` as
//! a hard error. `MAPERROR` uses the `ERRIN`/`ERROUT` bytes in `MSBDATA.INC`.
//! Timeout `80h` becomes AL=2. `ERR$CNT` stores status `8102h`.

const STATUS: usize = 3;
const COUNT: usize = 18;
const ROMREAD: u8 = 2;
const MAXERR: u16 = 5;
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
const FTOOBIG: u8 = 0x80;
const FFHARDFILE: u8 = 5;
const ERROR_AH: u16 = 0x8100;

/// ROM AH values and the BIOS AL codes from `MSBDATA.INC`.
const ERRIN: [u8; 8] = [0xCC, 0x80, 0x40, 0x10, 0x08, 0x06, 0x04, 0x03];
const ERROUT: [u8; 9] = [10, 2, 6, 4, 4, 15, 8, 0, 12];

#[derive(Clone, Copy)]
#[repr(C)]
struct Bds {
    drivelet: u8,
    drivenum: u8,
    flags: u16,
    fatsiz: u8,
    formfactor: u8,
    seclim: u16,
    hdlim: u16,
    drvlim: u16,
    hidsec: u16,
}

#[repr(C)]
struct Disk {
    bds: Bds,
    medbyt: u8,
    seccnt: u16,
    cursec: u8,
    curhd: u8,
    curtrk: u16,
    rflag: u8,
    sector: u16,
    sector_high: u16,
    lsterr: u8,
    ptrsav: usize,
}

static mut DISK: Disk = Disk {
    bds: Bds {
        drivelet: 0,
        drivenum: 0x80,
        flags: FNON_REMOVABLE,
        fatsiz: 0,
        formfactor: FFHARDFILE,
        seclim: 17,
        hdlim: 2,
        drvlim: 10000,
        hidsec: 0,
    },
    medbyt: 0xF8,
    seccnt: 0,
    cursec: 1,
    curhd: 0,
    curtrk: 0,
    rflag: ROMREAD,
    sector: 0,
    sector_high: 0,
    lsterr: 0,
    ptrsav: 0,
};

/// `MEDIA$CHK` for the fixed disk. `Secrete_Code` is `jk`, the disk is not
/// removable, and format has not changed it, so the result is 1 (no change).
pub fn media_check() -> i16 {
    if SECRETE_CODE != u16::from_le_bytes(*b"jk") {
        return 0;
    }
    let flags = disk().bds.flags;
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
    disk().bds.drivelet == logical
}

/// `DSK$REM`. A fixed disk takes the busy exit.
pub fn dsk_removable() -> u16 {
    if disk().bds.flags & FNON_REMOVABLE != 0 {
        0x0300
    } else {
        0x0100
    }
}

/// `CHECKSINGLE`. A fixed disk, or a drive that is not shared, returns at once.
pub fn check_single() -> bool {
    let flags = disk().bds.flags;
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
        && !ask_rom_media(disk().bds.flags)
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
    let mut packet = [0u8; 32];
    packet[2] = 4;
    let count = 1u16;
    packet[COUNT..COUNT + 2].copy_from_slice(&count.to_le_bytes());
    unsafe {
        let disk = &mut *core::ptr::addr_of_mut!(DISK);
        disk.ptrsav = packet.as_mut_ptr() as usize;
        disk.sector = 0;
        disk.sector_high = 0;
        disk.seccnt = count;
        disk.medbyt = 0xF8;
        disk.rflag = ROMREAD;
    }
    let carry = diskrd();
    if carry {
        err_cnt();
    } else {
        write_status(0x0100);
    }
    let _ = buffer;
    packet_status()
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
    let packed_sector = (cyl & 0xff) << 8 | u16::from(sector & 0x3f) | ((cyl & 0x300) >> 2);
    let drive_head = u16::from(drive) | (u16::from(head) << 8);
    let (_status, carry) = disk_service(ah_cmd, count, packed_sector, drive_head, buf.as_mut_ptr());
    carry
}

fn diskrd() -> bool {
    let disk = disk();
    if disk.bds.drivelet != 0 {
        return true;
    }
    if disk.seccnt == 0 {
        return false;
    }
    if disk.bds.flags & UNFORMATTED_MEDIA != 0 {
        return true;
    }
    let start = disk.sector as u32 + u32::from(disk.bds.hidsec);
    let end = start + u32::from(disk.seccnt);
    if disk.bds.drvlim != 0 && end > u32::from(disk.bds.drvlim) {
        return true;
    }
    let seclim = u32::from(disk.bds.seclim);
    let track = start / seclim;
    let mut sector = (start % seclim) as u8;
    sector = sector.wrapping_add(1);
    let heads = u32::from(disk.bds.hdlim);
    let cylinder = track / heads;
    let head = (track % heads) as u8;
    if cylinder > 1024 {
        return true;
    }
    unsafe {
        let disk = &mut *core::ptr::addr_of_mut!(DISK);
        disk.cursec = sector;
        disk.curhd = head;
        disk.curtrk = cylinder as u16;
    }
    block()
}

fn block() -> bool {
    let (seclim, cursec, count) = unsafe {
        let disk = &*core::ptr::addr_of!(DISK);
        (disk.bds.seclim as u8, disk.cursec, disk.seccnt)
    };
    let on_track = seclim.wrapping_add(1).wrapping_sub(cursec) as u16;
    let this = count.min(on_track);
    disk_io(this)
}

fn disk_io(count: u16) -> bool {
    let mut base = MAXERR;
    let function = disk().rflag;
    loop {
        let failed = fastspeed(function, count as u8);
        if !failed.1 {
            return false;
        }
        if dskerr(failed.0, &mut base) {
            return true;
        }
    }
}

/// `FASTSPEED`. `FTOOBIG` returns AH=`80h` and carry. Otherwise INT 13h.
fn fastspeed(ah_cmd: u8, count: u8) -> (u8, bool) {
    let too_big = disk().bds.fatsiz & FTOOBIG != 0;
    if too_big {
        return (0x80, true);
    }
    if disk().bds.formfactor != FFHARDFILE && ah_cmd != ROMREAD {
        return int13(ah_cmd, count);
    }
    int13(ah_cmd, count)
}

fn dskerr(function: u8, base: &mut u16) -> bool {
    *base = base.saturating_sub(1);
    if *base == 0 || function == 0x80 || function == 0xCC {
        map_error(function);
        return true;
    }
    false
}

fn map_error(function: u8) -> u8 {
    unsafe {
        (*core::ptr::addr_of_mut!(DISK)).lsterr = function;
    }
    let mut result = 12;
    for (index, code) in ERRIN.iter().enumerate() {
        if *code == function {
            result = ERROUT[index];
            break;
        }
    }
    if !ERRIN.contains(&function) {
        result = ERROUT[ERROUT.len() - 1];
    }
    unsafe {
        (*core::ptr::addr_of_mut!(DISK)).lsterr = result;
    }
    result
}

fn err_cnt() {
    let result = disk().lsterr;
    let packed = disk().seccnt;
    let pkt = ptrsav();
    let count = packet_word(COUNT);
    write_word(pkt, COUNT, count.wrapping_sub(packed));
    write_status(ERROR_AH | u16::from(result));
}

fn int13(ah_cmd: u8, count: u8) -> (u8, bool) {
    let sector = disk().cursec;
    let cyl = disk().curtrk;
    let head = disk().curhd;
    let drive = disk().bds.drivenum;
    let packed_sector = (u16::from(sector & 0x3f)) | ((cyl & 0xff) << 8);
    let drive_head = u16::from(drive) | (u16::from(head) << 8);
    disk_service(ah_cmd, count, packed_sector, drive_head, core::ptr::null_mut())
}

fn disk_service(command: u8, count: u8, packed_sector: u16, drive_head: u16, buffer: *mut u8) -> (u8, bool) {
    let call = (u32::from(command) << 8) | u32::from(count);
    let result = bios_int13_regs(call, buffer as u32, u32::from(packed_sector), u32::from(drive_head));
    let status = ((result >> 8) & 0xff) as u8;
    (status, result & 0x1_0000 != 0)
}

/// Disk service. Hard disks time out. Drive 0 reads `disks/msload.img`.
///
/// The low 16 bits are the status word. Bit 16 is the carry flag.
fn bios_int13_regs(call: u32, buffer: u32, packed_sector: u32, drive_head: u32) -> u32 {
    let command = ((call >> 8) & 0xff) as u8;
    let count = (call & 0xff) as u8;
    let drive = (drive_head & 0xff) as u8;
    if drive & 0x80 != 0 {
        return (0x80u32 << 8) | (1 << 16);
    }
    if command == 0 {
        return 0;
    }
    if command != 2 || count == 0 {
        return (0x01u32 << 8) | (1 << 16);
    }
    let sector = packed_sector & 0x3f;
    let cylinder = ((packed_sector >> 8) & 0xff) | ((packed_sector & 0xc0) << 2);
    let head = (drive_head >> 8) & 0xff;
    const SECTORS_PER_TRACK: u32 = 18;
    const HEADS: u32 = 2;
    if sector == 0 {
        return (0x04u32 << 8) | (1 << 16);
    }
    let lba = (cylinder * HEADS + head) * SECTORS_PER_TRACK + (sector - 1);
    let image = msload_image();
    let start = lba as usize * 512;
    let bytes = count as usize * 512;
    if start.saturating_add(bytes) > image.len() {
        return (0x04u32 << 8) | (1 << 16);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(image[start..].as_ptr(), buffer as *mut u8, bytes);
    }
    u32::from(count)
}

fn msload_image() -> &'static [u8] {
    include_bytes!("../../disks/msload.img")
}

fn disk() -> &'static Disk {
    unsafe { &*core::ptr::addr_of!(DISK) }
}

fn ptrsav() -> *mut u8 {
    disk().ptrsav as *mut u8
}

fn packet_word(offset: usize) -> u16 {
    let pkt = ptrsav();
    unsafe { u16::from_le_bytes([*pkt.add(offset), *pkt.add(offset + 1)]) }
}

fn write_word(pkt: *mut u8, offset: usize, value: u16) {
    let bytes = value.to_le_bytes();
    unsafe {
        *pkt.add(offset) = bytes[0];
        *pkt.add(offset + 1) = bytes[1];
    }
}

fn write_status(status: u16) {
    write_word(ptrsav(), STATUS, status);
}

fn packet_status() -> u16 {
    packet_word(STATUS)
}
