//! Hardware user-mode entry on each CPU. The architecture library owns
//! the register save/restore instructions; Rust owns the page and trap state.
mod pages;
#[cfg(feature = "tlb-selftest")]
pub(crate) mod tlb_test;
use crate::arch::frames::UserImage as Image;
use alloc::vec::Vec;
use core::{
    mem::size_of,
    ptr,
    sync::atomic::{AtomicU16, Ordering},
};
pub(crate) use pages::{AddressSpace, UserPageTable};
use x86::{
    Ring,
    dtables::{DescriptorTablePointer, lgdt, sgdt},
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
// The descriptor base identifies this CPU after a user trap, including when
// user GS/FS refer to application selectors.
#[repr(C, align(16))]
struct Local {
    gdt: [u64; 7],
    tss: [u8; 104],
    host_stack: usize,
    frame: *mut Frame,
    user_fp: *mut u8,
    host_fp: Floating,
}
impl Local {
    const EMPTY: Self = Self {
        gdt: [0; 7],
        tss: [0; 104],
        host_stack: 0,
        frame: ptr::null_mut(),
        user_fp: ptr::null_mut(),
        host_fp: Floating::INITIAL,
    };
}
static mut CPUS: [Local; crate::platform::MAX_CPUS] =
    [const { Local::EMPTY }; crate::platform::MAX_CPUS];
static APICS: [AtomicU16; crate::platform::MAX_CPUS] =
    [const { AtomicU16::new(u16::MAX) }; crate::platform::MAX_CPUS];

fn reschedule_slot(slot: usize) {
    if let Ok(id) = u8::try_from(APICS[slot].load(Ordering::Acquire)) {
        super::platform::reschedule(id);
    }
}

pub(super) fn init(slot: usize) {
    let apic = super::platform::boot_cpu().0;
    // Startup assigns each online CPU a unique slot before running any user
    // work. This CPU alone owns its descriptor bytes and trap return state.
    unsafe {
        let local = &mut *(&raw mut CPUS).cast::<Local>().add(slot);
        let long = cfg!(target_arch = "x86_64");
        local.gdt[1] = if long {
            0x0020_9a00_0000_0000
        } else {
            0x00cf_9a00_0000_ffff
        };
        local.gdt[2] = 0x00cf_9200_0000_ffff;
        local.gdt[3] = if long {
            0x0020_fa00_0000_0000
        } else {
            0x00cf_fa00_0000_ffff
        };
        local.gdt[4] = 0x00cf_f200_0000_ffff;
        let base = local.tss.as_ptr() as u64;
        local.gdt[5] =
            103 | ((base & 0xff_ffff) << 16) | (0x89 << 40) | ((base & 0xff00_0000) << 32);
        local.gdt[6] = base >> 32;
        #[cfg(target_arch = "x86")]
        ptr::write_unaligned(local.tss.as_mut_ptr().add(8).cast::<u16>(), 16);
        local.tss[102] = 104; // I/O bitmap beyond the segment limit
        lgdt(&DescriptorTablePointer {
            base: local.gdt.as_ptr(),
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
    APICS[slot].store(u16::from(apic), Ordering::Release);
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct StackChunk([u8; 16]);
pub(crate) struct Thread {
    regs: Frame,
    fp: Floating,
    kernel_stack: Vec<StackChunk>,
}
pub(crate) enum Stop {
    Call(usize),
    Slice,
    Fault(usize),
}
impl Thread {
    pub fn try_new(
        entry: usize,
        sp: usize,
        argc: usize,
        argv: usize,
    ) -> Result<Self, &'static str> {
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
        const STACK_CHUNKS: usize = 1024;
        let mut kernel_stack = Vec::new();
        kernel_stack
            .try_reserve_exact(STACK_CHUNKS)
            .map_err(|_| "insufficient memory for thread kernel stack")?;
        kernel_stack.resize(STACK_CHUNKS, StackChunk([0; 16]));
        Ok(Self {
            regs,
            fp: Floating::INITIAL,
            kernel_stack,
        })
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
    pub fn step_on(&mut self, slot: usize, table: &UserPageTable) -> Result<Stop, &'static str> {
        // IRET rejects noncanonical RIP/RSP before reaching userspace.
        #[cfg(target_arch = "x86_64")]
        if self.regs.sp as u64 >= dosrs_native_exec::USER_END
            || self.regs.ip as u64 >= dosrs_native_exec::USER_END
        {
            return Ok(Stop::Fault(13));
        }
        let (id, apic) = super::platform::boot_cpu();
        if !apic
            || APICS
                .get(slot)
                .is_none_or(|v| v.load(Ordering::Acquire) != u16::from(id))
            || !super::platform::enable_apic()
        {
            return Err("native thread requires its scheduled CPU and local APIC");
        }
        let _activation = table.acquire(slot)?;
        let host = pages::enter(table)?;
        // The scheduler exclusively owns this thread until this CPU traps. RSP0
        // (ESP0 on i686) names its kernel stack for every privilege transition.
        unsafe {
            let local = (&raw mut CPUS).cast::<Local>().add(slot);
            let top = self.kernel_stack.as_mut_ptr().add(self.kernel_stack.len()) as usize;
            ptr::write_unaligned(
                (&raw mut (*local).tss).cast::<u8>().add(4).cast::<usize>(),
                top,
            );
            let frame = (top - size_of::<Frame>()) as *mut Frame;
            frame.write(self.regs);
            let context = frame.cast::<usize>().sub(cpu::CONTEXT + 1);
            context.write_bytes(0, cpu::CONTEXT);
            context
                .add(cpu::CONTEXT)
                .write(trapret as *const () as usize);
            (*local).frame = &raw mut self.regs;
            (*local).user_fp = self.fp.0.as_mut_ptr();
            super::traps::use_user_gates(true);
            super::platform::user_timer(true);
            cpu::save((&raw mut (*local).host_fp).cast());
            cpu::restore((*local).user_fp);
            cpu::swtch(&raw mut (*local).host_stack, context as usize);
            super::platform::user_timer(false);
            super::traps::use_user_gates(false);
            (*local).frame = ptr::null_mut();
            (*local).user_fp = ptr::null_mut();
        }
        host.restore();
        Ok(match self.regs.vector {
            64 => Stop::Call(self.regs.regs[cpu::AX]),
            0xf0 | 0xf1 | 0xff => Stop::Slice,
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
        let mut descriptor = DescriptorTablePointer::<u64> {
            base: ptr::null(),
            limit: 0,
        };
        sgdt(&mut descriptor);
        let local = descriptor.base as *mut Local;
        if (*local).frame.is_null() || (*frame).cs & 3 != 3 {
            super::halt()
        }
        cpu::save((*local).user_fp);
        cpu::restore((&raw const (*local).host_fp).cast());
        (*local).frame.write(frame.read());
        if matches!((*frame).vector, 0xf0 | 0xf1) {
            super::platform::user_eoi();
        }
        let mut discarded = 0;
        cpu::swtch(&mut discarded, (*local).host_stack);
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
