//! Disk read from `BIOS/MSDISK.ASM` for one fixed-disk sector.
//!
//! There is no disk controller under QEMU `-kernel`. `FASTSPEED` executes
//! INT 13h. The handler returns carry and AH=`80h`. `DSKERR` treats `80h` as
//! a hard error. `MAPERROR` uses the `ERRIN`/`ERROUT` bytes in `MSBDATA.INC`.
//! Timeout `80h` becomes AL=2. `ERR$CNT` stores status `8102h`.

use core::arch::{asm, global_asm};

const STATUS: usize = 3;
const COUNT: usize = 18;
const ROMREAD: u8 = 2;
const MAXERR: u16 = 5;
const FNON_REMOVABLE: u16 = 0x0001;
const FTOOBIG: u8 = 0x80;
const FFHARDFILE: u8 = 5;
const ERROR_AH: u16 = 0x8100;

/// ROM AH values and the BIOS AL codes from `MSBDATA.INC`.
const ERRIN: [u8; 8] = [0xCC, 0x80, 0x40, 0x10, 0x08, 0x06, 0x04, 0x03];
const ERROUT: [u8; 9] = [10, 2, 6, 4, 4, 15, 8, 0, 12];

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

fn diskrd() -> bool {
    let disk = disk();
    if disk.bds.drivelet != 0 {
        return true;
    }
    if disk.seccnt == 0 {
        return false;
    }
    if disk.bds.flags & 0x200 != 0 {
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
    let (seclim, cursec, count, fixed) = unsafe {
        let disk = &*core::ptr::addr_of!(DISK);
        (
            disk.bds.seclim as u8,
            disk.cursec,
            disk.seccnt,
            disk.bds.flags & FNON_REMOVABLE != 0,
        )
    };
    let on_track = seclim.wrapping_add(1).wrapping_sub(cursec) as u16;
    let this = if fixed { count.min(on_track) } else { count.min(on_track) };
    disk_io(this)
}

fn disk_io(count: u16) -> bool {
    let mut bp = MAXERR;
    let ah = disk().rflag;
    loop {
        let failed = fastspeed(ah, count as u8);
        if !failed.1 {
            return false;
        }
        if dskerr(failed.0, &mut bp) {
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

fn dskerr(ah: u8, bp: &mut u16) -> bool {
    *bp = bp.saturating_sub(1);
    if *bp == 0 || ah == 0x80 || ah == 0xCC {
        map_error(ah);
        return true;
    }
    false
}

fn map_error(ah: u8) -> u8 {
    unsafe {
        (*core::ptr::addr_of_mut!(DISK)).lsterr = ah;
    }
    let mut al = 12;
    for (index, code) in ERRIN.iter().enumerate() {
        if *code == ah {
            al = ERROUT[index];
            break;
        }
    }
    if !ERRIN.contains(&ah) {
        al = ERROUT[ERROUT.len() - 1];
    }
    unsafe {
        (*core::ptr::addr_of_mut!(DISK)).lsterr = al;
    }
    al
}

fn err_cnt() {
    let al = disk().lsterr;
    let cx = disk().seccnt;
    let pkt = ptrsav();
    let count = packet_word(COUNT);
    write_word(pkt, COUNT, count.wrapping_sub(cx));
    write_status(ERROR_AH | u16::from(al));
}

fn int13(ah_cmd: u8, count: u8) -> (u8, bool) {
    let sector = disk().cursec;
    let cyl = disk().curtrk;
    let head = disk().curhd;
    let drive = disk().bds.drivenum;
    let mut cx = ((cyl & 0xff) << 8) | u16::from(sector & 0x3f) | ((cyl & 0x300) >> 2);
    let _ = cx;
    cx = (u16::from(sector & 0x3f)) | ((cyl & 0xff) << 8);
    let dx = u16::from(drive) | (u16::from(head) << 8);
    let ax_in = u16::from(ah_cmd) << 8 | u16::from(count);
    #[cfg(target_arch = "x86")]
    let (ax, flags) = unsafe {
        let ax: u32;
        let flags: u32;
        asm!(
            "mov ax, {ax:x}",
            "mov cx, {cx:x}",
            "mov dx, {dx:x}",
            "int 0x13",
            "pushfd",
            "pop ebx",
            ax = in(reg) ax_in,
            cx = in(reg) cx,
            dx = in(reg) dx,
            lateout("eax") ax,
            lateout("ebx") flags,
            lateout("ecx") _,
            lateout("edx") _,
        );
        (ax, flags)
    };
    #[cfg(target_arch = "x86_64")]
    let (ax, flags) = unsafe {
        let ax: u64;
        let flags: u64;
        asm!(
            "mov ax, {ax:x}",
            "mov cx, {cx:x}",
            "mov dx, {dx:x}",
            "int 0x13",
            "pushfq",
            "pop r8",
            ax = in(reg) ax_in,
            cx = in(reg) cx,
            dx = in(reg) dx,
            lateout("rax") ax,
            lateout("r8") flags,
            lateout("rcx") _,
            lateout("rdx") _,
        );
        (ax as u32, flags as u32)
    };
    (((ax >> 8) & 0xff) as u8, (flags & 1) != 0)
}

#[no_mangle]
extern "C" fn bios_int13() {
    // No controller. Timeout, carry set by the gate.
}

#[cfg(target_arch = "x86")]
global_asm!(
    r#"
.section .text, "ax"
.code32
.global int13_gate
int13_gate:
    mov ah, 0x80
    or dword ptr [esp + 8], 1
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
global_asm!(
    r#"
.section .text, "ax"
.code64
.global int13_gate
int13_gate:
    mov ah, 0x80
    or qword ptr [rsp + 16], 1
    iretq
"#
);

unsafe extern "C" {
    fn int13_gate();
}

pub fn int13_gate_addr() -> usize {
    int13_gate as *const () as usize
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
