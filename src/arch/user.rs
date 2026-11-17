//! Hardware user-mode entry on the boot CPU. The architecture library owns
//! the register save/restore instructions; Rust owns the page and trap state.
mod pages;
use core::{mem::size_of, ptr};
use dosrs_native_exec::Image;
pub(crate) use pages::AddressSpace;
use x86::{
    Ring,
    dtables::{DescriptorTablePointer, lgdt},
    segmentation::{self as seg, SegmentSelector},
};

#[cfg(target_arch = "x86")]
mod cpu {
    pub use core::arch::x86::{_fxrstor as restore, _fxsave as save};
    unsafe extern "C" {
        pub fn swtch(old: *mut usize, new: usize);
    }
    pub const GPRS: usize = 12;
    pub const AX: usize = 7;
    pub const CONTEXT: usize = 4;
}
#[cfg(target_arch = "x86_64")]
mod cpu {
    pub use core::arch::x86_64::{_fxrstor64 as restore, _fxsave64 as save};
    unsafe extern "sysv64" {
        pub fn swtch(old: *mut usize, new: usize);
    }
    pub const GPRS: usize = 15;
    pub const AX: usize = 0;
    pub const CONTEXT: usize = 6;
}
unsafe extern "C" {
    fn trapret();
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Frame {
    regs: [usize; cpu::GPRS],
    vector: usize,
    error: usize,
    ip: usize,
    cs: usize,
    flags: usize,
    sp: usize,
    ss: usize,
}
#[repr(C, align(16))]
struct Floating([u8; 512]);
impl Floating {
    const INITIAL: Self = {
        let mut f = [0u8; 512];
        f[0] = 0x7f;
        f[1] = 3;
        f[24] = 0x80;
        f[25] = 0x1f;
        Self(f)
    };
}
#[repr(C, align(16))]
struct Stack([usize; 4096]);
static mut STACK: Stack = Stack([0; 4096]);
static mut TSS: [u8; 104] = [0; 104];
static mut GDT: [u64; 7] = [0; 7];
static mut HOST_STACK: usize = 0;
static mut RETURNED: *mut Frame = ptr::null_mut();
static mut USER_FP: *mut u8 = ptr::null_mut();
static mut HOST_FP: Floating = Floating::INITIAL;

pub(super) fn init() {
    // One BSP GDT and TSS, installed before the IDT. APs have separate tables.
    unsafe {
        let long = cfg!(target_arch = "x86_64");
        GDT[1] = if long {
            0x0020_9a00_0000_0000
        } else {
            0x00cf_9a00_0000_ffff
        };
        GDT[2] = 0x00cf_9200_0000_ffff;
        GDT[3] = if long {
            0x0020_fa00_0000_0000
        } else {
            0x00cf_fa00_0000_ffff
        };
        GDT[4] = 0x00cf_f200_0000_ffff;
        let base = &raw mut TSS as u64;
        GDT[5] = 103 | ((base & 0xff_ffff) << 16) | (0x89 << 40) | ((base & 0xff00_0000) << 32);
        GDT[6] = base >> 32;
        let stack = (&raw const STACK as usize) + size_of::<Stack>();
        ptr::write_unaligned((&raw mut TSS).cast::<u8>().add(4).cast::<usize>(), stack);
        #[cfg(target_arch = "x86")]
        ptr::write_unaligned((&raw mut TSS).cast::<u8>().add(8).cast::<u16>(), 16);
        TSS[102] = 104; // no user-accessible I/O bitmap
        lgdt(&DescriptorTablePointer {
            base: (&raw const GDT).cast::<u64>(),
            limit: 55,
        });
        x86::dtables::load_ldtr(SegmentSelector::new(0, Ring::Ring0));
        seg::load_cs(SegmentSelector::new(1, Ring::Ring0));
        let data = SegmentSelector::new(2, Ring::Ring0);
        seg::load_ds(data);
        seg::load_es(data);
        seg::load_ss(data);
        seg::load_fs(data);
        seg::load_gs(data);
        x86::task::load_tr(SegmentSelector::new(5, Ring::Ring0));
    }
}
pub(crate) struct Thread {
    regs: Frame,
    fp: Floating,
}
pub(crate) enum Stop {
    Call(usize),
    Slice,
    Fault(usize),
}
impl Thread {
    pub fn new(entry: usize, sp: usize, argc: usize, argv: usize) -> Self {
        let mut regs = Frame {
            regs: [0; cpu::GPRS],
            vector: 0,
            error: 0,
            ip: entry,
            cs: 27,
            flags: 0x202,
            sp,
            ss: 35,
        };
        #[cfg(target_arch = "x86_64")]
        {
            regs.regs[6] = argc;
            regs.regs[5] = argv;
        }
        #[cfg(target_arch = "x86")]
        {
            let _ = (argc, argv);
            regs.regs[10] = 35;
            regs.regs[11] = 35;
        }
        Self {
            regs,
            fp: Floating::INITIAL,
        }
    }
    pub fn result(&mut self, value: isize) {
        self.regs.regs[cpu::AX] = value as usize;
    }
    pub fn args(&self, image: &Image, count: usize) -> Option<[usize; 3]> {
        #[cfg(target_arch = "x86_64")]
        {
            let _ = (image, count);
            Some([self.regs.regs[6], self.regs.regs[5], self.regs.regs[3]])
        }
        #[cfg(target_arch = "x86")]
        {
            let mut args = [0; 3];
            for (i, a) in args.iter_mut().take(count).enumerate() {
                let mut bytes = [0; 4];
                image
                    .read(self.regs.sp.checked_add(4 + i * 4)?, &mut bytes)
                    .ok()?;
                *a = u32::from_le_bytes(bytes) as usize;
            }
            Some(args)
        }
    }
    pub fn step(&mut self, image: &mut AddressSpace) -> Result<Stop, &'static str> {
        #[cfg(target_arch = "x86")]
        if self.regs.sp > dosrs_native_exec::END {
            return Ok(Stop::Fault(12));
        }
        // IRET rejects noncanonical RIP/RSP before reaching userspace.
        #[cfg(target_arch = "x86_64")]
        if self.regs.sp as u64 >= dosrs_native_exec::USER_END
            || self.regs.ip as u64 >= dosrs_native_exec::USER_END
        {
            return Ok(Stop::Fault(13));
        }
        if !super::platform::boot_cpu().1 || !super::platform::enable_apic() {
            return Err("native processes require the local APIC");
        }
        let host = pages::enter(image)?;
        // Assembly consumes this context then IRETs to a Frame, on a stack
        // owned exclusively by this synchronous activation of the BSP.
        unsafe {
            let top = (&raw mut STACK as usize) + size_of::<Stack>();
            let frame = (top - size_of::<Frame>()) as *mut Frame;
            frame.write(self.regs);
            let context = frame.cast::<usize>().sub(cpu::CONTEXT + 1);
            context.write_bytes(0, cpu::CONTEXT);
            context
                .add(cpu::CONTEXT)
                .write(trapret as *const () as usize);
            RETURNED = &raw mut self.regs;
            USER_FP = self.fp.0.as_mut_ptr();
            super::traps::use_user_gates(true);
            super::platform::user_timer(true);
            cpu::save((&raw mut HOST_FP).cast());
            cpu::restore(USER_FP);
            cpu::swtch(&raw mut HOST_STACK, context as usize);
            super::platform::user_timer(false);
            super::traps::use_user_gates(false);
            RETURNED = ptr::null_mut();
            USER_FP = ptr::null_mut();
        }
        host.restore();
        Ok(match self.regs.vector {
            64 => Stop::Call(self.regs.regs[cpu::AX]),
            0xf0 | 0xff => Stop::Slice,
            fault => Stop::Fault(fault),
        })
    }
}
fn from_user(frame: *const Frame) -> ! {
    // xv6 has saved all integer registers and installed flat data segments.
    // Reset DF before Rust copies the frame; preserve the application's flags
    // in its IRET record. The kernel is compiled with soft-float.
    #[cfg(target_arch = "x86_64")]
    x86::bits64::rflags::set(x86::bits64::rflags::RFlags::new());
    #[cfg(target_arch = "x86")]
    unsafe {
        x86::bits32::eflags::set(x86::bits32::eflags::EFlags::from_bits_truncate(2));
    }
    unsafe {
        if RETURNED.is_null() || (*frame).cs & 3 != 3 {
            super::halt()
        }
        cpu::save(USER_FP);
        cpu::restore((&raw const HOST_FP).cast());
        RETURNED.write(frame.read());
        if (*frame).vector == 0xf0 {
            super::platform::user_eoi();
        }
        let mut discarded = 0;
        cpu::swtch(&mut discarded, HOST_STACK);
        super::halt()
    }
}
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
extern "sysv64" fn trap(frame: *const Frame) -> ! {
    from_user(frame)
}
#[cfg(target_arch = "x86")]
#[unsafe(no_mangle)]
extern "C" fn trap(frame: *const Frame) -> ! {
    from_user(frame)
}
