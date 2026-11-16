//! Native DOS INT 21h dispatch.
//!
//! The logical frame, the dispatch table, AH=30h, the shared in-range stub,
//! and the out-of-range check all live here. The arch gate is the only caller
//! of [`entry`].

use crate::arch::GateToken;
mod files;

/// Low 16 bits of AX, BX, or CX as the DOS ABI freezes them.
///
/// This is not a `u64`. A handler cannot pass a full register into [`LogicalFrame::store_ax`].
#[derive(Clone, Copy)]
struct DosWord(u16);

/// Low byte of AX.
///
/// Distinct from [`FunctionCode`] so a function number cannot be stored as AL.
#[derive(Clone, Copy)]
struct ResultByte(u8);

impl ResultByte {
    const ZERO: Self = Self(0);
}

/// AH function number.
///
/// Distinct from [`ResultByte`] and from [`DosWord`].
#[derive(Clone, Copy)]
struct FunctionCode(u8);

impl FunctionCode {
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
    status: u64,
    handle: u64,
    count: u64,
    buffer: u64,
    source: u64,
    destination: u64,
    base: u64,
    data_segment: u64,
    extra_segment: u64,
}

/// The untyped GPR image the mapper copies.
///
/// Flags, the return address, and the x86_64 interrupt stack pointer are absent.
#[derive(Clone, Copy)]
pub(crate) struct NativeRegs {
    pub status: u64,
    pub handle: u64,
    pub count: u64,
    pub buffer: u64,
    pub source: u64,
    pub destination: u64,
    pub base: u64,
    pub data_segment: u64,
    pub extra_segment: u64,
}

impl LogicalFrame {
    /// Builds a frame from the CPU's incoming bits.
    ///
    /// Only the arch gate can mint `token`. Handlers do not call this.
    pub(crate) fn from_native(token: &GateToken, regs: NativeRegs) -> Self {
        token.seal();
        Self {
            status: regs.status,
            handle: regs.handle,
            count: regs.count,
            buffer: regs.buffer,
            source: regs.source,
            destination: regs.destination,
            base: regs.base,
            data_segment: regs.data_segment,
            extra_segment: regs.extra_segment,
        }
    }

    /// Returns the GPR image the mapper writes back.
    ///
    /// The flags image is not in this value.
    pub(crate) fn native(&self, token: &GateToken) -> NativeRegs {
        token.seal();
        NativeRegs {
            status: self.status,
            handle: self.handle,
            count: self.count,
            buffer: self.buffer,
            source: self.source,
            destination: self.destination,
            base: self.base,
            data_segment: self.data_segment,
            extra_segment: self.extra_segment,
        }
    }

    fn function(&self) -> FunctionCode {
        FunctionCode((self.status >> 8) as u8)
    }

    /// Replaces bits 0..16 of AX. Bits 16..63 stay as they were.
    fn store_ax(&mut self, word: DosWord) {
        self.status = (self.status & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..16 of BX. Bits 16..63 stay as they were.
    fn store_bx(&mut self, word: DosWord) {
        self.handle = (self.handle & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..16 of CX. Bits 16..63 stay as they were.
    fn store_cx(&mut self, word: DosWord) {
        self.count = (self.count & !0xFFFF) | u64::from(word.0);
    }

    /// Replaces bits 0..8 of AX. AH and bits 16..63 stay as they were.
    fn set_al(&mut self, result: ResultByte) {
        self.status = (self.status & !0xFF) | u64::from(result.0);
    }

    /// Replaces bits 0..8 of DX. DH and bits 16..63 stay as they were.
    fn set_dl(&mut self, data_low: u8) {
        self.buffer = (self.buffer & !0xFF) | u64::from(data_low);
    }

    /// Replaces bits 0..16 of DX. Bits 16..63 stay as they were.
    fn store_dx(&mut self, word: DosWord) {
        self.buffer = (self.buffer & !0xFFFF) | u64::from(word.0);
    }
}

/// One pointer per AH from 0 through the last slot.
///
/// Every slot starts as [`in_range_stub`]. A translated function replaces one slot.
/// Filesystem calls use the mounted DOS namespace before this register-only table.
const DISPATCH: [fn(&mut LogicalFrame); 0x6D] = {
    let mut slots = [in_range_stub as fn(&mut LogicalFrame); 0x6D];
    slots[0x02] = std_con_output as fn(&mut LogicalFrame);
    slots[0x29] = parse_fcb as fn(&mut LogicalFrame);
    slots[0x09] = std_con_string_output as fn(&mut LogicalFrame);
    slots[0x44] = ioctl as fn(&mut LogicalFrame);
    slots[0x48] = alloc_memory as fn(&mut LogicalFrame);
    slots[0x49] = free_memory as fn(&mut LogicalFrame);
    slots[0x2A] = get_date as fn(&mut LogicalFrame);
    slots[0x2B] = set_date as fn(&mut LogicalFrame);
    slots[0x2C] = get_time as fn(&mut LogicalFrame);
    slots[0x2D] = set_time as fn(&mut LogicalFrame);
    slots[0x2E] = set_verify_on_write as fn(&mut LogicalFrame);
    slots[0x30] = get_version as fn(&mut LogicalFrame);
    slots[0x33] = set_ctrl_c_trapping as fn(&mut LogicalFrame);
    slots[0x54] = get_verify_on_write as fn(&mut LogicalFrame);
    slots[0x59] = get_extended_error as fn(&mut LogicalFrame);
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
    if !files::call(token, frame) {
        dispatch(frame);
    }
}
fn dispatch(frame: &mut LogicalFrame) {
    let function = frame.function();
    if function.above_max() {
        out_of_range(frame);
        return;
    }
    DISPATCH[function.index()](frame);
}

/// AH=59h. AX is the last error. BH is the class, BL the action, CH the locus.
fn get_extended_error(frame: &mut LogicalFrame) {
    let (status, handle, count) = crate::dos::ms_code::get();
    frame.store_ax(DosWord(status));
    frame.store_bx(DosWord(handle));
    frame.store_cx(DosWord(count));
}

/// AH=48h. BX is the paragraph count. AX is the memory segment.
fn alloc_memory(frame: &mut LogicalFrame) {
    match crate::dos::alloc::alloc(frame.handle as u16) {
        Ok(seg) => frame.store_ax(DosWord(seg)),
        Err((code, max)) => {
            frame.store_ax(DosWord(code));
            frame.store_bx(DosWord(max));
        }
    }
}

/// AH=49h. ES is the memory segment, one paragraph above the header.
fn free_memory(frame: &mut LogicalFrame) {
    frame.store_ax(DosWord(crate::dos::alloc::free(frame.extra_segment as u16)));
}

/// AH=29h. DS:SI is the text. ES:DI is the FCB. AL is the control bits.
fn parse_fcb(frame: &mut LogicalFrame) {
    let flags = (frame.status & 0xff) as u8;
    let (result, source) = crate::dos::parse::parse(
        flags,
        frame.source as *const u8,
        frame.destination as *mut u8,
    );
    frame.set_al(ResultByte(result));
    frame.source = source as u64;
}

/// AH=2. DL is the character. The console device receives that byte.
fn std_con_output(frame: &mut LogicalFrame) {
    crate::dos::cpmio2::out_char((frame.buffer & 0xff) as u8);
}

/// AH=9. DX is the linear address of a `$`-terminated string.
fn std_con_string_output(frame: &mut LogicalFrame) {
    crate::dos::cpmio::write_dollar_string(frame.buffer as u32);
}

/// DOS date. 1980-01-01 is the unset clock, a Tuesday.
use core::sync::atomic::{AtomicU32, Ordering};
static DATE: AtomicU32 = AtomicU32::new((1980 << 16) | 0x0101);
static TIME: AtomicU32 = AtomicU32::new(0);

/// AH=44h. AL=0 and AL=8 are the slices of `$IOCTL` this boot uses.
fn ioctl(frame: &mut LogicalFrame) {
    let result = (frame.status & 0xff) as u8;
    let (status, buffer, al_out) = crate::dos::ioctl::on_frame(result, frame.handle as u16);
    frame.store_ax(DosWord(if result == 8 {
        u16::from(al_out)
    } else {
        status
    }));
    frame.store_dx(DosWord(buffer));
    if result == 8 && status == 0 {
        frame.set_al(ResultByte(al_out));
    }
}

/// AH=2Ah. CX is the year, DH the month, DL the day, AL the weekday.
fn get_date(frame: &mut LogicalFrame) {
    let (year, month, day, weekday) = current_date();
    frame.store_cx(DosWord(year));
    frame.store_dx(DosWord((u16::from(month) << 8) | u16::from(day)));
    frame.set_al(ResultByte(weekday));
}

/// AH=2Bh. AL is 0 when the date is stored, and 0xFF when it is rejected.
fn set_date(frame: &mut LogicalFrame) {
    let year = frame.count as u16;
    let month = (frame.buffer >> 8) as u8;
    let day = frame.buffer as u8;
    frame.set_al(ResultByte(store_date(year, month, day)));
}

/// AH=2Ch. CH hours, CL minutes, DH seconds, DL hundredths.
fn get_time(frame: &mut LogicalFrame) {
    let (hour, minute, second, hundredth) = current_time();
    frame.store_cx(DosWord((u16::from(hour) << 8) | u16::from(minute)));
    frame.store_dx(DosWord((u16::from(second) << 8) | u16::from(hundredth)));
}

/// AH=2Dh. AL is 0 when the time is stored, and 0xFF when it is rejected.
fn set_time(frame: &mut LogicalFrame) {
    let hour = (frame.count >> 8) as u8;
    let minute = frame.count as u8;
    let second = (frame.buffer >> 8) as u8;
    let hundredth = frame.buffer as u8;
    frame.set_al(ResultByte(store_time(hour, minute, second, hundredth)));
}

/// AH=2Eh. `$SET_VERIFY_ON_WRITE`. AL bit 0 is the flag.
fn set_verify_on_write(frame: &mut LogicalFrame) {
    crate::dos::getset::set_verify((frame.status & 0xff) as u8);
}

/// AH=54h. `$GET_VERIFY_ON_WRITE`. AL comes back as the flag.
fn get_verify_on_write(frame: &mut LogicalFrame) {
    frame.set_al(ResultByte(crate::dos::getset::get_verify()));
}

/// AH=33h. `$SET_CTRL_C_TRAPPING`.
fn set_ctrl_c_trapping(frame: &mut LogicalFrame) {
    let result = (frame.status & 0xff) as u8;
    let data_low = (frame.buffer & 0xff) as u8;
    if result == 6 {
        frame.store_bx(DosWord(crate::version::WORD));
        frame.store_dx(DosWord(0));
        return;
    }
    let (new_al, new_dl) = crate::dos::disp::set_ctrl_c(result, data_low);
    frame.set_al(ResultByte(new_al));
    frame.set_dl(new_dl);
}

/// AH=30h. AL=1 requests DOS residency flags in BH.
///
/// Returns 7.10 in AX, zero in CX and the OEM identifier (or residency flags) in BH.
/// Leaves the upper bits of those registers alone. Does not write DX, SI, DI, BP, DS, or ES.
fn get_version(frame: &mut LogicalFrame) {
    let (ax, bx, cx) = crate::dos::getset::VERSION;
    let flags_requested = frame.status as u8 == 1;
    frame.store_ax(DosWord(ax));
    frame.store_bx(DosWord(if flags_requested { 0 } else { bx }));
    frame.store_cx(DosWord(cx));
}

/// Shared body for every empty in-range slot.
///
/// Sets AL to 0 and leaves every other register alone. Not a list of function numbers.
fn in_range_stub(frame: &mut LogicalFrame) {
    frame.set_al(ResultByte::ZERO);
}

/// AH above [`MAXCOM`].
///
/// Sets AL to 0. Leaves AH and every other register alone. Does not build an [`ErrorCode`].
fn out_of_range(frame: &mut LogicalFrame) {
    frame.set_al(ResultByte::ZERO);
}

pub fn current_date() -> (u16, u8, u8, u8) {
    let [hi, lo, month, day] = DATE.load(Ordering::Relaxed).to_be_bytes();
    let year = u16::from_be_bytes([hi, lo]);
    (
        year,
        month,
        day,
        crate::dos::time::set_date(year, month, day)
            .expect("validated DOS date")
            .weekday,
    )
}

pub fn store_date(year: u16, month: u8, day: u8) -> u8 {
    if crate::dos::time::set_date(year, month, day).is_err() {
        return 0xff;
    }
    DATE.store(
        (u32::from(year) << 16) | u32::from(u16::from_be_bytes([month, day])),
        Ordering::Relaxed,
    );
    0
}

pub fn current_time() -> (u8, u8, u8, u8) {
    TIME.load(Ordering::Relaxed).to_be_bytes().into()
}

pub fn store_time(hour: u8, minute: u8, second: u8, hundredth: u8) -> u8 {
    if crate::dos::time::set_time(hour, minute, second, hundredth).is_err() {
        return 0xff;
    }
    TIME.store(
        u32::from_be_bytes([hour, minute, second, hundredth]),
        Ordering::Relaxed,
    );
    0
}

// Exercise the native service adapter with a typed register image. The QEMU
// hardware-gate test separately drives the actual CPU interrupt and its flags.
pub(crate) fn version_call(carry: bool) -> (u16, u16, u16, bool) {
    let mut frame = LogicalFrame {
        status: 0x30a5,
        handle: 0x1234,
        count: 0x5678,
        buffer: 0,
        source: 0,
        destination: 0,
        base: 0,
        data_segment: 0,
        extra_segment: 0,
    };
    dispatch(&mut frame);
    (
        frame.status as u16,
        frame.handle as u16,
        frame.count as u16,
        carry,
    )
}
