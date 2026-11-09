//! Device entry from `BIOS/MSBIO1.ASM`.
//!
//! `STRATEGY` saves the request packet. `CON$IN` and `AUX0$IN` index the
//! command tables and call the driver. `OUTCHR` stays the INT 29h gate.

static mut PTRSAV: usize = 0;
static mut AUXNUM: u8 = 0;

pub(crate) fn packet() -> *mut u8 {
    unsafe { PTRSAV as *mut u8 }
}

pub(crate) fn auxnum() -> u8 {
    unsafe { AUXNUM }
}

/// `STRATEGY`. BX is the packet offset and ES is its segment. Here the packet
/// is one pointer.
pub fn strategy(packet: *mut u8) {
    unsafe { PTRSAV = packet as usize }
}

/// `CON$IN`. SI selects `CONTBL`.
pub fn con_in() {
    super::mscon::con_in();
}

/// `AUX0$IN`. AL is 0, then `AUXTBL`.
pub fn aux0_in() {
    unsafe { AUXNUM = 0 }
    super::msaux::aux0_in();
}

/// `GETDX`. DX is the word at `AUXNUM`.
pub(crate) fn get_dx() -> u16 {
    u16::from(auxnum())
}

pub fn run_con(packet: *mut u8) {
    strategy(packet);
    con_in();
}

pub fn run_aux0(packet: *mut u8) {
    strategy(packet);
    aux0_in();
}
