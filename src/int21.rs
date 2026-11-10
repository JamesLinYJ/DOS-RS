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
    slots[0x30] = get_version as fn(&mut LogicalFrame);
    slots[0x4B] = exec_program as fn(&mut LogicalFrame);
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
