//! BSP address space used between entry to user mode and the next trap.
//! Only the selected process's leaf entries carry the user bit.
use dosrs_native_exec::{BASE, Image, PAGE};
use x86::controlregs::*;

#[repr(C, align(4096))]
struct Table([u64; 512]);
impl Table {
    const EMPTY: Self = Self([0; 512]);
}
static mut INITIALIZED: bool = false;
static mut LEAVES: Table = Table::EMPTY;
static mut DIRECTORIES: [Table; 4] = [const { Table::EMPTY }; 4];
static mut DIRECTORY_POINTERS: Table = Table::EMPTY;
#[cfg(target_arch = "x86_64")]
static mut ROOT: Table = Table::EMPTY;
pub(super) struct Host {
    cr0: Cr0,
    cr3: u64,
    cr4: Cr4,
    efer: Option<u64>,
    sysenter: Option<u64>,
}

pub(super) fn enter(image: &mut Image) -> Result<Host, &'static str> {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::__cpuid;
    let features = __cpuid(1).edx;
    const REQUIRED: u32 = (1 << 6) | (1 << 24) | (1 << 25);
    if features & REQUIRED != REQUIRED {
        return Err("native processes require PAE, FXSR and SSE");
    }
    let ext = if __cpuid(0x80000000).eax >= 0x80000001 {
        __cpuid(0x80000001).edx
    } else {
        0
    };
    let nx = ext & (1 << 20) != 0;
    let has_efer = ext & ((1 << 11) | (1 << 20) | (1 << 29)) != 0;
    // The kernel and BSP stack are identity mapped by both DOS-RS boot paths.
    // Until a high-memory kernel mapping exists, reject firmware placement
    // beyond the low identity map or in the process's virtual address window.
    let low = |address: usize| {
        (address as u64) < 0x1_0000_0000 && !(BASE..dosrs_native_exec::END).contains(&address)
    };
    if !low(&nx as *const _ as usize)
        || !low(enter as *const () as usize)
        || !low(&raw const DIRECTORY_POINTERS as usize)
        || image
            .pages()
            .any(|(_, p)| !low(p.bytes().as_ptr() as usize))
    {
        return Err("kernel outside supported low memory map");
    }
    // The scheduler invokes this only on the BSP with interrupts disabled and
    // with no active user address space. APs retain their own page-table root.
    unsafe {
        let host = Host {
            cr0: cr0(),
            cr3: cr3(),
            cr4: cr4(),
            efer: has_efer.then(|| x86::msr::rdmsr(0xc0000080)),
            sysenter: (features & (1 << 11) != 0).then(|| x86::msr::rdmsr(0x174)),
        };
        if host
            .cr4
            .intersects(Cr4::CR4_ENABLE_LA57 | Cr4::CR4_ENABLE_PCID)
        {
            return Err("unsupported host paging mode");
        }
        if !INITIALIZED {
            for i in 0..4 {
                DIRECTORY_POINTERS.0[i] = (&raw const DIRECTORIES[i] as u64)
                    | if cfg!(target_arch = "x86_64") { 7 } else { 1 };
                for j in 0..512 {
                    DIRECTORIES[i].0[j] = ((i as u64 * 512 + j as u64) << 21) | 0x83;
                }
            }
            DIRECTORIES[BASE >> 30].0[(BASE >> 21) & 511] = (&raw const LEAVES as u64) | 7;
            INITIALIZED = true;
        }
        LEAVES.0 = [0; 512];
        for (addr, mapping) in image.pages_mut() {
            let mut flags = 5;
            if mapping.writable {
                flags |= 2;
            }
            if nx && !mapping.executable {
                flags |= 1 << 63;
            }
            LEAVES.0[(addr - BASE) / PAGE] = mapping.frame_address() as u64 | flags;
        }
        #[cfg(target_arch = "x86_64")]
        let root = {
            ROOT.0[0] = (&raw const DIRECTORY_POINTERS as u64) | 7;
            &raw const ROOT as u64
        };
        #[cfg(target_arch = "x86")]
        let root = &raw const DIRECTORY_POINTERS as u64;
        #[cfg(target_arch = "x86")]
        if host.cr0.contains(Cr0::CR0_ENABLE_PAGING) {
            cr0_write(host.cr0 - Cr0::CR0_ENABLE_PAGING);
        }
        cr4_write(
            (host.cr4 | Cr4::CR4_ENABLE_PAE | Cr4::CR4_ENABLE_SSE)
                - Cr4::CR4_ENABLE_GLOBAL_PAGES
                - Cr4::CR4_ENABLE_FSGSBASE
                - Cr4::CR4_ENABLE_OS_XSAVE
                - Cr4::CR4_ENABLE_PROTECTION_KEY,
        );
        // INT 40h is the only userspace syscall entry. Save firmware MSR settings.
        if let Some(efer) = host.efer {
            x86::msr::wrmsr(0xc0000080, (efer & !1) | if nx { 1 << 11 } else { 0 });
        }
        if host.sysenter.is_some() {
            x86::msr::wrmsr(0x174, 0);
        }
        cr3_write(root);
        cr0_write(
            (host.cr0 | Cr0::CR0_ENABLE_PAGING | Cr0::CR0_WRITE_PROTECT)
                - Cr0::CR0_TASK_SWITCHED
                - Cr0::CR0_EMULATE_COPROCESSOR,
        );
        Ok(host)
    }
}
impl Host {
    pub(super) fn restore(self) {
        // No instruction executes in the user alias on the trap/return path.
        unsafe {
            #[cfg(target_arch = "x86")]
            cr0_write(cr0() - Cr0::CR0_ENABLE_PAGING);
            cr3_write(self.cr3);
            cr4_write(self.cr4);
            if let Some(efer) = self.efer {
                x86::msr::wrmsr(0xc0000080, efer);
            }
            if let Some(cs) = self.sysenter {
                x86::msr::wrmsr(0x174, cs);
            }
            cr0_write(self.cr0);
        }
    }
}
