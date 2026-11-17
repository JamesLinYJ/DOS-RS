//! Each process owns its PAE or four-level page-table root. All threads of the
//! process load the same root. Other processes only share supervisor mappings.
use crate::arch::frames::UserImage as Image;
use alloc::{sync::Arc, vec::Vec};
#[cfg(target_arch = "x86")]
use core::arch::x86::__cpuid;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__cpuid;
use core::ops::Range;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use dosrs_native_exec::{ANON_BASE, ANON_END, BASE, END, PAGE};
use x86::controlregs::*;

#[repr(C, align(4096))]
struct Table([AtomicU64; 512]);
impl Table {
    const EMPTY: Self = Self([const { AtomicU64::new(0) }; 512]);
}

pub(crate) struct AddressSpace {
    image: Image,
    table: Option<Arc<UserPageTable>>,
}
impl From<Image> for AddressSpace {
    fn from(image: Image) -> Self {
        Self { image, table: None }
    }
}
impl core::ops::Deref for AddressSpace {
    type Target = Image;
    fn deref(&self) -> &Image {
        &self.image
    }
}
impl AddressSpace {
    pub(crate) fn write(&mut self, address: usize, bytes: &[u8]) -> Result<(), &'static str> {
        self.image.write(address, bytes)
    }
    pub(crate) fn prepare(&mut self) -> Result<(), &'static str> {
        if self.table.is_none() {
            self.table = Some(UserPageTable::build(&self.image, 0..0)?);
        }
        Ok(())
    }
    pub(crate) fn map_anonymous(&mut self, pages: usize) -> Result<usize, &'static str> {
        let base = self.image.map_anonymous(pages)?;
        match UserPageTable::build(&self.image, 0..0) {
            Ok(table) => {
                self.invalidate();
                self.table = Some(table);
                Ok(base)
            }
            Err(error) => {
                // The old CR3 never contained these newly allocated frames.
                self.image.unmap_anonymous(base, pages)?;
                Err(error)
            }
        }
    }

    pub(crate) fn unmap_anonymous(
        &mut self,
        base: usize,
        pages: usize,
    ) -> Result<(), &'static str> {
        self.image.validate_unmap(base, pages)?;
        // Allocate the replacement while all old mappings and their backing
        // frames remain intact. A failed rebuild leaves the old root usable.
        let next = UserPageTable::build(&self.image, base..base + pages * PAGE)?;
        self.invalidate();
        self.table = Some(next);
        self.image.unmap_anonymous(base, pages)
    }

    pub(crate) fn page_table(&self) -> Arc<UserPageTable> {
        self.table
            .as_ref()
            .expect("unprepared process address space")
            .clone()
    }
    /// Stop admitting new slices and wait until every CPU has restored its host
    /// CR3. Stale queued handles cannot reactivate the retired mappings.
    pub(crate) fn invalidate(&mut self) {
        if let Some(table) = self.table.take() {
            table.retire();
        }
    }
}
impl Drop for AddressSpace {
    fn drop(&mut self) {
        self.invalidate();
    }
}
fn identity_mapped(address: usize) -> bool {
    (address as u64) < 0x1_0000_0000
        && !(BASE..END).contains(&address)
        && !(ANON_BASE..ANON_END).contains(&address)
}
const RETIRED: usize = 1 << (usize::BITS - 1);
const LONG: bool = cfg!(target_arch = "x86_64");
const PDPT: usize = LONG as usize;
const FIRST_PD: usize = PDPT + 1;

pub(crate) struct UserPageTable {
    tables: Vec<Table>,
    active: AtomicUsize,
}
impl UserPageTable {
    fn root(&self) -> usize {
        self.tables.as_ptr() as usize
    }

    fn build(image: &Image, removed: Range<usize>) -> Result<Arc<Self>, &'static str> {
        let retained = || {
            image
                .pages()
                .filter(|(address, _)| !removed.contains(address))
        };
        // Reserve the exact number of branches before embedding their physical
        // addresses. Sorted ELF pages in the same subtree allocate one branch.
        let mut count = FIRST_PD + 4;
        let mut prev = u64::MAX;
        for (address, _) in retained() {
            let addr = address as u64;
            count += usize::from(addr >> 21 != prev >> 21);
            count += usize::from(addr >= 1 << 32 && addr >> 30 != prev >> 30);
            count += usize::from(addr >= 1 << 39 && addr >> 39 != prev >> 39);
            prev = addr;
        }
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(count)
            .map_err(|_| "insufficient memory for user page tables")?;
        tables.resize_with(count, || Table::EMPTY);
        if tables
            .iter()
            .any(|p| !identity_mapped(p as *const Table as usize))
        {
            return Err("page tables outside the kernel identity map");
        }
        let base = tables.as_ptr() as usize;
        let phys = |i: usize| (base + i * PAGE) as u64;
        if LONG {
            *tables[0].0[0].get_mut() = phys(PDPT) | 7;
        }
        for i in 0..4 {
            *tables[PDPT].0[i].get_mut() = phys(FIRST_PD + i) | if LONG { 7 } else { 1 };
            for j in 0..512 {
                let addr = (i * 512 + j) << 21;
                if identity_mapped(addr) {
                    *tables[FIRST_PD + i].0[j].get_mut() = addr as u64 | 0x83;
                }
            }
        }
        let nx =
            __cpuid(0x8000_0000).eax >= 0x8000_0001 && __cpuid(0x8000_0001).edx & (1 << 20) != 0;
        let mut next = FIRST_PD + 4;
        let mut pdpt = PDPT;
        let mut pd = FIRST_PD;
        let mut pt = 0;
        prev = u64::MAX;
        for (address, mapping) in retained() {
            let addr = address as u64;
            if addr >= 1 << 39 && addr >> 39 != prev >> 39 {
                pdpt = next;
                next += 1;
                *tables[0].0[((addr >> 39) & 511) as usize].get_mut() = phys(pdpt) | 7;
            }
            if addr >> 30 != prev >> 30 {
                if addr < 1 << 32 {
                    pd = FIRST_PD + (addr >> 30) as usize;
                } else {
                    pd = next;
                    next += 1;
                    *tables[pdpt].0[((addr >> 30) & 511) as usize].get_mut() = phys(pd) | 7;
                }
            }
            if addr >> 21 != prev >> 21 {
                pt = next;
                next += 1;
                *tables[pd].0[((addr >> 21) & 511) as usize].get_mut() = phys(pt) | 7;
            }
            let frame = mapping.frame_address();
            if !identity_mapped(frame) {
                return Err("user frame outside the kernel identity map");
            }
            let flags = 5
                | if mapping.writable { 2 } else { 0 }
                | if nx && !mapping.executable {
                    1 << 63
                } else {
                    0
                };
            *tables[pt].0[((addr >> 12) & 511) as usize].get_mut() = frame as u64 | flags;
            prev = addr;
        }
        Arc::try_new(Self {
            tables,
            active: AtomicUsize::new(0),
        })
        .map_err(|_| "insufficient memory for process address space")
    }

    pub(super) fn acquire(&self, slot: usize) -> Result<Activation<'_>, &'static str> {
        let bit = 1 << slot;
        self.active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                (v & (RETIRED | bit) == 0).then_some(v | bit)
            })
            .map_err(|_| "retired or already active address space")?;
        Ok(Activation { table: self, bit })
    }
    fn retire(&self) {
        let mask = self.active.fetch_or(RETIRED, Ordering::AcqRel);
        for cpu in 0..crate::platform::MAX_CPUS {
            if mask & (1 << cpu) != 0 {
                super::reschedule_slot(cpu);
            }
        }
        while self.active.load(Ordering::Acquire) != RETIRED {
            core::hint::spin_loop();
        }
    }
}
pub(super) struct Activation<'a> {
    table: &'a UserPageTable,
    bit: usize,
}
impl Drop for Activation<'_> {
    fn drop(&mut self) {
        self.table.active.fetch_and(!self.bit, Ordering::Release);
    }
}
pub(super) struct Host {
    cr0: Cr0,
    cr3: u64,
    cr4: Cr4,
    efer: Option<u64>,
    sysenter: Option<u64>,
}

pub(super) fn enter(table: &UserPageTable) -> Result<Host, &'static str> {
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
    if !identity_mapped(&nx as *const _ as usize) || !identity_mapped(enter as *const () as usize) {
        return Err("kernel outside supported low memory map");
    }
    // The local CPU disables interrupts in kernel mode. Its saved controls stay
    // on its own scheduler stack until this activation traps and leaves the CR3.
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
        cr3_write(table.root() as u64);
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
            #[cfg(feature = "tlb-selftest")]
            assert_eq!(
                cr3(),
                self.cr3,
                "host CR3 must be restored before TLB acknowledgement"
            );
        }
    }
}
