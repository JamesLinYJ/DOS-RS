//! Static ELF process images for the i686 and x86_64 DOS-RS kernels.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use object::{
    LittleEndian, elf,
    read::elf::{FileHeader, ProgramHeader},
};

include!(concat!(env!("OUT_DIR"), "/calls.rs"));

pub const PAGE: usize = 4096;
pub const BASE: usize = 0x6000_0000;
pub const END: usize = BASE + 0x20_0000;
pub const STACK: usize = END - 16 * PAGE;
const LIMIT: usize = STACK - PAGE;

#[derive(Clone, Copy)]
pub enum Machine {
    I386,
    Amd64,
}

#[repr(C, align(4096))]
struct Frame([u8; PAGE]);

// Leave kernel heap space for descriptors, pending I/O and filesystem transactions.
const MAX_RESIDENT_PAGES: usize = (2 << 20) / PAGE;
static RESIDENT: AtomicUsize = AtomicUsize::new(0);
pub struct Mapping {
    frame: Vec<Frame>,
    pub writable: bool,
    pub executable: bool,
}
pub struct Image {
    pages: Vec<(usize, Mapping)>,
    pub entry: usize,
}
pub type Result<T> = core::result::Result<T, &'static str>;

impl Image {
    pub fn load(file: &[u8], machine: Machine) -> Result<Self> {
        match machine {
            Machine::I386 => Self::parse::<elf::FileHeader32<LittleEndian>>(file, elf::EM_386),
            Machine::Amd64 => Self::parse::<elf::FileHeader64<LittleEndian>>(file, elf::EM_X86_64),
        }
    }

    fn parse<E: FileHeader<Endian = LittleEndian>>(file: &[u8], machine: u16) -> Result<Self> {
        let le = LittleEndian;
        let h = E::parse(file).map_err(|_| "invalid ELF header")?;
        if h.e_ident().data != elf::ELFDATA2LSB
            || h.e_machine(le) != machine
            || h.e_type(le) != elf::ET_EXEC
            || h.e_version(le) != 1
            || usize::from(h.e_ehsize(le)) != size_of::<E>()
        {
            return Err("unsupported ELF executable");
        }
        let headers = h
            .program_headers(le, file)
            .map_err(|_| "invalid ELF program headers")?;
        let entry = usize::try_from(h.e_entry(le).into()).map_err(|_| "invalid ELF entry")?;
        let mut image = Self {
            pages: Vec::new(),
            entry,
        };
        let mut prior_end = BASE;
        let mut executable_entry = false;
        for p in headers {
            match p.p_type(le) {
                elf::PT_LOAD => (),
                elf::PT_INTERP | elf::PT_DYNAMIC | elf::PT_TLS => {
                    return Err("ELF requires a runtime linker or TLS");
                }
                _ => continue,
            }
            let (off, filesz) = p.file_range(le);
            let address: u64 = p.p_vaddr(le).into();
            let memory: u64 = p.p_memsz(le).into();
            let align: u64 = p.p_align(le).into();
            if filesz > memory
                || (align > 1 && (!align.is_power_of_two() || address % align != off % align))
                || address % PAGE as u64 != off % PAGE as u64
            {
                return Err("invalid ELF segment geometry");
            }
            if memory == 0 {
                continue;
            }
            let start = usize::try_from(address).map_err(|_| "invalid ELF address")?;
            let end = address
                .checked_add(memory)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or("invalid ELF address")?;
            if start < prior_end || end > LIMIT {
                return Err("ELF segment is outside the process image");
            }
            prior_end = end;
            let flags = p.p_flags(le);
            let write = flags & elf::PF_W != 0;
            let exec = flags & elf::PF_X != 0;
            let contents = p.data(le, file).map_err(|_| "truncated ELF segment")?;
            for base in (start / PAGE * PAGE..end).step_by(PAGE) {
                let m = image.append_page(base)?;
                m.writable |= write;
                m.executable |= exec;
                if m.writable && m.executable {
                    return Err("ELF needs a writable code page");
                }
                let first = start.max(base);
                let last = (base + PAGE).min(start + contents.len());
                if first < last {
                    m.frame[0].0[first - base..last - base]
                        .copy_from_slice(&contents[first - start..last - start]);
                }
            }
            executable_entry |= exec && (start..end).contains(&entry);
        }
        if !executable_entry {
            return Err("entry is outside executable code");
        }
        for addr in (STACK..END).step_by(PAGE) {
            image.append_page(addr)?.writable = true;
        }
        Ok(image)
    }

    // PT_LOAD segments are already checked for monotonically increasing addresses.
    // Adjacent segments may share their boundary page.
    fn append_page(&mut self, addr: usize) -> Result<&mut Mapping> {
        if self.pages.last().map(|p| p.0) != Some(addr) {
            self.pages
                .try_reserve(1)
                .map_err(|_| "insufficient page metadata")?;
            self.pages.push((addr, Mapping::zero()?));
        }
        Ok(&mut self.pages.last_mut().unwrap().1)
    }
    fn index(&self, base: usize) -> Result<usize> {
        self.pages
            .binary_search_by_key(&base, |p| p.0)
            .map_err(|_| "unmapped user address")
    }
    pub fn pages(&self) -> impl Iterator<Item = (usize, &Mapping)> {
        self.pages.iter().map(|(addr, mapping)| (*addr, mapping))
    }
    pub fn pages_mut(&mut self) -> impl Iterator<Item = (usize, &mut Mapping)> {
        self.pages
            .iter_mut()
            .map(|(addr, mapping)| (*addr, mapping))
    }
    pub fn check(&self, addr: usize, len: usize, write: bool) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let end = addr
            .checked_add(len)
            .filter(|&end| addr >= BASE && end <= END)
            .ok_or("invalid user address")?;
        for base in (addr / PAGE * PAGE..end).step_by(PAGE) {
            if !self.pages[self.index(base)?].1.writable && write {
                return Err("unmapped or protected user address");
            }
        }
        Ok(())
    }
    pub fn read(&self, mut addr: usize, mut out: &mut [u8]) -> Result<()> {
        self.check(addr, out.len(), false)?;
        while !out.is_empty() {
            let offset = addr % PAGE;
            let n = out.len().min(PAGE - offset);
            out[..n].copy_from_slice(
                &self.pages[self.index(addr - offset)?].1.bytes()[offset..offset + n],
            );
            addr += n;
            out = &mut out[n..];
        }
        Ok(())
    }
    pub fn write(&mut self, mut addr: usize, mut input: &[u8]) -> Result<()> {
        self.check(addr, input.len(), true)?;
        while !input.is_empty() {
            let offset = addr % PAGE;
            let n = input.len().min(PAGE - offset);
            let index = self.index(addr - offset)?;
            self.pages[index].1.frame[0].0[offset..offset + n].copy_from_slice(&input[..n]);
            addr += n;
            input = &input[n..];
        }
        Ok(())
    }
}
impl Mapping {
    fn zero() -> Result<Self> {
        RESIDENT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < MAX_RESIDENT_PAGES).then_some(n + 1)
            })
            .map_err(|_| "native process memory limit reached")?;
        let mut mapping = Self {
            frame: Vec::new(),
            writable: false,
            executable: false,
        };
        mapping
            .frame
            .try_reserve_exact(1)
            .map_err(|_| "insufficient physical memory")?;
        mapping.frame.push(Frame([0; PAGE]));
        Ok(mapping)
    }
    /// Hardware may write this frame only during the exclusive activation of its image.
    pub fn frame_address(&mut self) -> usize {
        self.frame.as_mut_ptr().addr()
    }
    pub fn bytes(&self) -> &[u8; PAGE] {
        &self.frame[0].0
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        RESIDENT.fetch_sub(1, Ordering::Relaxed);
    }
}
