//! Native descriptor table and the UEFI PI register image saved by the CPU library.
use crate::int21::{LogicalFrame, NativeRegs};
use x86::dtables::{DescriptorTablePointer, lidt};

#[repr(C)]
struct Templates {
    base: usize,
    stride: usize,
    after: usize,
}
unsafe extern "efiapi" {
    fn AsmGetTemplateAddressMap(map: &mut Templates);
}
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
static mErrorCodeFlag: [u32; 8] = [0x60227d00, 0, 0, 0, 0, 0, 0, 0];
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
static mDoFarReturnFlag: usize = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    low: u16,
    cs: u16,
    ist: u8,
    attr: u8,
    mid: u16,
    #[cfg(target_pointer_width = "64")]
    high: u32,
    #[cfg(target_pointer_width = "64")]
    reserved: u32,
}
impl Gate {
    const ZERO: Self = Self {
        low: 0,
        cs: 0,
        ist: 0,
        attr: 0,
        mid: 0,
        #[cfg(target_pointer_width = "64")]
        high: 0,
        #[cfg(target_pointer_width = "64")]
        reserved: 0,
    };
    fn new(addr: usize, cs: u16) -> Self {
        Self {
            low: addr as u16,
            cs,
            attr: 0x8e,
            mid: (addr >> 16) as u16,
            #[cfg(target_pointer_width = "64")]
            high: (addr >> 32) as u32,
            ..Self::ZERO
        }
    }
}
static mut BSP_IDT: [Gate; 256] = [Gate::ZERO; 256];
static mut AP_IDT: [Gate; 256] = [Gate::ZERO; 256];
pub(crate) fn install_interrupts() {
    let mut map = Templates {
        base: 0,
        stride: 0,
        after: 0,
    };
    // The CPU library fills three native-sized fields. Tables become immutable
    // before AP publication. The BSP keeps its firmware CS, APs use our GDT.
    unsafe {
        AsmGetTemplateAddressMap(&mut map);
        for n in 0..256 {
            let address = map.base + n * map.stride;
            BSP_IDT[n] = Gate::new(address, x86::segmentation::cs().bits());
            AP_IDT[n] = Gate::new(address, if cfg!(target_arch = "x86") { 8 } else { 24 });
        }
        lidt(&DescriptorTablePointer {
            base: (&raw const BSP_IDT).cast::<Gate>(),
            limit: (size_of::<[Gate; 256]>() - 1) as u16,
        });
    }
}
pub(crate) fn load_interrupts() {
    // Startup release publishes the fully initialized table before any AP reads it.
    unsafe {
        lidt(&DescriptorTablePointer {
            base: (&raw const AP_IDT).cast::<Gate>(),
            limit: (size_of::<[Gate; 256]>() - 1) as u16,
        });
    }
}
#[repr(C)]
struct Context {
    error: usize,
    fx: [u8; 512],
    debug: [usize; 6],
    control: [usize; 5],
    #[cfg(target_pointer_width = "64")]
    cr8: usize,
    flags: usize,
    ldtr: usize,
    tr: usize,
    gdt: [usize; 2],
    idt: [usize; 2],
    ip: usize,
    gs: usize,
    fs: usize,
    es: usize,
    ds: usize,
    cs: usize,
    ss: usize,
    di: usize,
    si: usize,
    bp: usize,
    sp: usize,
    bx: usize,
    dx: usize,
    cx: usize,
    ax: usize,
    #[cfg(target_pointer_width = "64")]
    extended: [usize; 8],
}
#[unsafe(no_mangle)]
extern "efiapi" fn CommonExceptionHandler(vector: usize, context: &mut Context) {
    if vector != 0x21 {
        // UART only: exception reporting must not borrow a terminal or the scheduler.
        for b in b"CPU exception " {
            super::outb(0x3f8, *b);
        }
        for shift in [4, 0] {
            super::outb(0x3f8, b"0123456789abcdef"[(vector >> shift) & 15]);
        }
        super::halt()
    }
    let gate = super::GateToken(());
    let mut frame = LogicalFrame::from_native(
        &gate,
        NativeRegs {
            status: context.ax as u64,
            handle: context.bx as u64,
            count: context.cx as u64,
            buffer: context.dx as u64,
            source: context.si as u64,
            destination: context.di as u64,
            base: context.bp as u64,
            data_segment: context.ds as u64,
            extra_segment: context.es as u64,
        },
    );
    crate::int21::entry(&gate, &mut frame);
    let r = frame.native(&gate);
    context.ax = r.status as usize;
    context.bx = r.handle as usize;
    context.cx = r.count as usize;
    context.dx = r.buffer as usize;
    context.si = r.source as usize;
    context.di = r.destination as usize;
    context.bp = r.base as usize;
}
