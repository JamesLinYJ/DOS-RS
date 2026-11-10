//! DOS INT 21h for this pull request.
//!
//! The logical frame, the dispatch table, AH=30h, the shared in-range stub,
//! and the out-of-range check all live here. The arch gate is the only caller
//! of [`entry`].

use crate::arch::GateToken;

/// Low 16 bits of AX, BX, or CX as the DOS ABI freezes them.
///
/// This is not a `u64`. A handler cannot pass a full register into [`LogicalFrame::store_ax`].
#[derive(Clone, Copy)]
struct DosWord(u16);

/// Low byte of AX.
///
/// Distinct from [`Ah`] so a function number cannot be stored as AL.
#[derive(Clone, Copy)]
struct Al(u8);

impl Al {
    const ZERO: Self = Self(0);
}

/// AH function number.
///
/// Distinct from [`Al`] and from [`DosWord`].
#[derive(Clone, Copy)]
struct Ah(u8);

impl Ah {
    /// True when AH is above the bound derived from [`DISPATCH`].
    fn above_max(self) -> bool {
        self.0 > MAXCOM
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// A DOS error code from `INC/ERROR.INC`.
///
/// Distinct from [`DosWord`]. AH=30h and an out-of-range AH must not build one.
/// `error_invalid_function` is 1, and these paths do not return it.
#[allow(dead_code)]
struct ErrorCode(u16);

/// CPU registers after the arch gate has mapped the trap, and before it maps back.
///
/// This is the `user_environ` analogue. The gate holds one value on its stack for
/// the duration of `int 0x21`. DOS code sees this value and does not see the CPU
/// push. Slots are 64-bit on both targets. There is no flags field, so a handler
/// cannot read or write carry.
pub(crate) struct LogicalFrame {
    ax: u64,
    bx: u64,
    cx: u64,
    dx: u64,
    si: u64,
    di: u64,
    bp: u64,
    ds: u64,
    es: u64,
}

/// The untyped GPR image the mapper copies.
///
/// Flags, the return address, and the x86_64 interrupt stack pointer are absent.
#[derive(Clone, Copy)]
pub(crate) struct NativeRegs {
    pub ax: u64,
    pub bx: u64,
    pub cx: u64,
    pub dx: u64,
    pub si: u64,
    pub di: u64,
    pub bp: u64,
    pub ds: u64,
    pub es: u64,
}

impl LogicalFrame {
    /// Builds a frame from the CPU's incoming bits.
    ///
    /// Only the arch gate can mint `token`. Handlers do not call this.
    pub(crate) fn from_native(token: &GateToken, regs: NativeRegs) -> Self {
        token.seal();
        Self {
            ax: regs.ax,
            bx: regs.bx,
            cx: regs.cx,
            dx: regs.dx,
            si: regs.si,
            di: regs.di,
            bp: regs.bp,
            ds: regs.ds,
            es: regs.es,
        }
    }

    /// Returns the GPR image the mapper writes back.
    ///
    /// The flags image is not in this value.
    pub(crate) fn native(&self, token: &GateToken) -> NativeRegs {
        token.seal();
        NativeRegs {
            ax: self.ax,
            bx: self.bx,
            cx: self.cx,
            dx: self.dx,
            si: self.si,
            di: self.di,
            bp: self.bp,
            ds: self.ds,
            es: self.es,
        }
    }

    fn ah(&self) -> Ah {
        Ah((self.ax >> 8) as u8)
    }

    /// Replaces bits 0..16 of AX. Bits 16..63 stay as they were.
    fn store_ax(&mut self, word: DosWord) {
        self.ax = (self.ax & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..16 of BX. Bits 16..63 stay as they were.
    fn store_bx(&mut self, word: DosWord) {
        self.bx = (self.bx & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..16 of CX. Bits 16..63 stay as they were.
    fn store_cx(&mut self, word: DosWord) {
        self.cx = (self.cx & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..8 of AX. AH and bits 16..63 stay as they were.
    fn set_al(&mut self, al: Al) {
        self.ax = (self.ax & !0xFF) | u64::from(al.0);
    }

    /// Replaces bits 0..8 of DX. DH and bits 16..63 stay as they were.
    fn set_dl(&mut self, dl: u8) {
        self.dx = (self.dx & !0xFF) | u64::from(dl);
    }

    /// Replaces bits 0..16 of DX. Bits 16..63 stay as they were.
    fn store_dx(&mut self, word: DosWord) {
        self.dx = (self.dx & !0xFFFF) | u64::from(word.0);
    }
}

/// Major 4 in AL, minor 0 in AH. The word is `0x0004`.
const VERSION_AX: DosWord = DosWord(0x0004);
/// BH is OEM `0xFF`. BL is the high serial byte, which this image leaves at 0.
const OEM_BX: DosWord = DosWord(0xFF00);
/// Low 16 bits of the serial. This image leaves them at 0.
const SERIAL_CX: DosWord = DosWord(0x0000);

/// One pointer per AH from 0 through the last slot.
///
/// Every slot starts as [`in_range_stub`]. A translated function replaces one slot.
/// The index is the function number. This array is the only catalog.
const DISPATCH: [fn(&mut LogicalFrame); 0x6D] = {
    let mut slots = [in_range_stub as fn(&mut LogicalFrame); 0x6D];
    slots[0x02] = std_con_output as fn(&mut LogicalFrame);
    slots[0x19] = get_default_drive as fn(&mut LogicalFrame);
    slots[0x09] = std_con_string_output as fn(&mut LogicalFrame);
    slots[0x2A] = get_date as fn(&mut LogicalFrame);
    slots[0x2B] = set_date as fn(&mut LogicalFrame);
    slots[0x2C] = get_time as fn(&mut LogicalFrame);
    slots[0x2D] = set_time as fn(&mut LogicalFrame);
    slots[0x2E] = set_verify_on_write as fn(&mut LogicalFrame);
    slots[0x30] = get_version as fn(&mut LogicalFrame);
    slots[0x33] = set_ctrl_c_trapping as fn(&mut LogicalFrame);
    slots[0x4B] = exec_program as fn(&mut LogicalFrame);
    slots[0x54] = get_verify_on_write as fn(&mut LogicalFrame);
    slots
};

/// Last legal AH. Derived from [`DISPATCH`], then checked against the DOS 4.0 bound.
const MAXCOM: u8 = (DISPATCH.len() - 1) as u8;

const _: () = assert!(MAXCOM == 0x6C);

/// The one INT 21h entry behind the gate.
///
/// `token` is minted only in `arch`, so `kmain` cannot call this. AH above [`MAXCOM`]
/// clears AL and returns. Every other AH calls the one table. This function does not
/// write flags and does not touch an INDOS counter, because this module has no such counter.
pub(crate) fn entry(token: &GateToken, frame: &mut LogicalFrame) {
    token.seal();
    let ah = frame.ah();
    if ah.above_max() {
        out_of_range(frame);
        return;
    }
    DISPATCH[ah.index()](frame);
}

/// AH=19h. AL is the default drive. This image leaves it at 0, which is A.
fn get_default_drive(frame: &mut LogicalFrame) {
    frame.set_al(Al(0));
}

/// AH=2. DL is the character. The console device receives that byte.
fn std_con_output(frame: &mut LogicalFrame) {
    crate::dos::cpmio2::out_char((frame.dx & 0xff) as u8);
}

/// AH=9. DX is the linear address of a `$`-terminated string.
fn std_con_string_output(frame: &mut LogicalFrame) {
    crate::dos::cpmio::write_dollar_string(frame.dx as u32);
}

/// AH=4Bh. AL is the exec function. 0, 1, and 3 load. 2 is rejected.
fn exec_program(frame: &mut LogicalFrame) {
    let al = (frame.ax & 0xff) as u8;
    if !crate::dos::exec::function_ok(al) {
        frame.set_al(Al(1));
        return;
    }
    if !crate::dos::exec::image_has_command(frame.dx as u32 as *const u8) {
        frame.set_al(Al(2));
    }
}

/// DOS date. 1980-01-01 is the unset clock, a Tuesday.
static mut YEAR: u16 = 1980;
static mut MONTH: u8 = 1;
static mut DAY: u8 = 1;
static mut HOUR: u8 = 0;
static mut MINUTE: u8 = 0;
static mut SECOND: u8 = 0;
static mut HUNDREDTH: u8 = 0;

/// AH=2Ah. CX is the year, DH the month, DL the day, AL the weekday.
fn get_date(frame: &mut LogicalFrame) {
    let (year, month, day) = unsafe { (YEAR, MONTH, DAY) };
    frame.store_cx(DosWord(year));
    frame.store_dx(DosWord((u16::from(month) << 8) | u16::from(day)));
    frame.set_al(Al(weekday(year, month, day)));
}

/// AH=2Bh. AL is 0 when the date is stored, and 0xFF when it is rejected.
fn set_date(frame: &mut LogicalFrame) {
    let year = frame.cx as u16;
    let month = (frame.dx >> 8) as u8;
    let day = frame.dx as u8;
    if date_ok(year, month, day) {
        unsafe {
            YEAR = year;
            MONTH = month;
            DAY = day;
        }
        frame.set_al(Al(0));
    } else {
        frame.set_al(Al(0xff));
    }
}

/// AH=2Ch. CH hours, CL minutes, DH seconds, DL hundredths.
fn get_time(frame: &mut LogicalFrame) {
    let (hour, minute, second, hundredth) = unsafe { (HOUR, MINUTE, SECOND, HUNDREDTH) };
    frame.store_cx(DosWord((u16::from(hour) << 8) | u16::from(minute)));
    frame.store_dx(DosWord((u16::from(second) << 8) | u16::from(hundredth)));
}

/// AH=2Dh. AL is 0 when the time is stored, and 0xFF when it is rejected.
fn set_time(frame: &mut LogicalFrame) {
    let hour = (frame.cx >> 8) as u8;
    let minute = frame.cx as u8;
    let second = (frame.dx >> 8) as u8;
    let hundredth = frame.dx as u8;
    if hour < 24 && minute < 60 && second < 60 && hundredth < 100 {
        unsafe {
            HOUR = hour;
            MINUTE = minute;
            SECOND = second;
            HUNDREDTH = hundredth;
        }
        frame.set_al(Al(0));
    } else {
        frame.set_al(Al(0xff));
    }
}

/// Sakamoto. 0 is Sunday, matching INT 21h AH=2Ah.
fn weekday(year: u16, month: u8, day: u8) -> u8 {
    let table = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = i32::from(year);
    let m = i32::from(month);
    if m < 3 {
        y -= 1;
    }
    let w = (y + y / 4 - y / 100 + y / 400 + table[(m - 1) as usize] + i32::from(day)).rem_euclid(7);
    w as u8
}

fn date_ok(year: u16, month: u8, day: u8) -> bool {
    if year < 1980 || year > 2099 || month == 0 || month > 12 || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [0u8, 31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    day <= days[month as usize]
}

/// AH=2Eh. `$SET_VERIFY_ON_WRITE`. AL bit 0 is the flag.
fn set_verify_on_write(frame: &mut LogicalFrame) {
    crate::dos::getset::set_verify((frame.ax & 0xff) as u8);
}

/// AH=54h. `$GET_VERIFY_ON_WRITE`. AL comes back as the flag.
fn get_verify_on_write(frame: &mut LogicalFrame) {
    frame.set_al(Al(crate::dos::getset::get_verify()));
}

/// AH=33h. `$SET_CTRL_C_TRAPPING`.
fn set_ctrl_c_trapping(frame: &mut LogicalFrame) {
    let al = (frame.ax & 0xff) as u8;
    let dl = (frame.dx & 0xff) as u8;
    let (new_al, new_dl) = crate::dos::disp::set_ctrl_c(al, dl);
    frame.set_al(Al(new_al));
    frame.set_dl(new_dl);
}

/// AH=30h. AL is ignored.
///
/// Writes AX=`0x0004`, BX=`0xFF00`, and CX=`0x0000` into the low 16 bits.
/// Leaves the upper bits of those registers alone. Does not write DX, SI, DI, BP, DS, or ES.
fn get_version(frame: &mut LogicalFrame) {
    frame.store_ax(VERSION_AX);
    frame.store_bx(OEM_BX);
    frame.store_cx(SERIAL_CX);
}

/// Shared body for every empty in-range slot.
///
/// Sets AL to 0 and leaves every other register alone. Not a list of function numbers.
fn in_range_stub(frame: &mut LogicalFrame) {
    frame.set_al(Al::ZERO);
}

/// AH above [`MAXCOM`].
///
/// Sets AL to 0. Leaves AH and every other register alone. Does not build an [`ErrorCode`].
fn out_of_range(frame: &mut LogicalFrame) {
    frame.set_al(Al::ZERO);
}
