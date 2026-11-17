//! Static ELF process images for the i686 and x86_64 DOS-RS kernels.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
use alloc::vec::Vec;
use object::{
    LittleEndian, elf,
    read::elf::{FileHeader, ProgramHeader},
};

include!(concat!(env!("OUT_DIR"), "/calls.rs"));

pub const PAGE: usize = 4096;
pub const BASE: usize = 0x6000_0000;
pub const END: usize = BASE + 0x20_0000;
/// Sparse anonymous mappings keep the original ELF and stack ABI intact.
pub const ANON_BASE: usize = 0x6100_0000;
pub const ANON_END: usize = 0x8000_0000;
pub const STACK: usize = END - 16 * PAGE;
const LIMIT: usize = STACK - PAGE;
/// The kernel identity-maps the low four GiB. Sparse ELF64 segments can use
/// the rest of the lower canonical half in four-level paging mode.
pub const HIGH_USER_START: u64 = 1 << 32;
pub const USER_END: u64 = 1 << 47;

#[derive(Clone, Copy)]
pub enum Machine {
    I386,
    Amd64,
}

/// A page is owned by exactly one mapping. Releasing it must return its frame
/// to the provider. A live mapping may also be accessed by another user CPU.
pub trait Frame: Sized {
    fn allocate_zeroed() -> Result<Self>;
    /// A snapshot for rejecting oversized requests before reserving metadata.
    fn available() -> usize;
    fn frame_address(&self) -> usize;
    fn read(&self, offset: usize, out: &mut [u8]);
    fn write(&mut self, offset: usize, input: &[u8]);
}

pub struct Mapping<F> {
    frame: F,
    pub writable: bool,
    pub executable: bool,
}
pub struct Image<F: Frame> {
    pages: Vec<(usize, Mapping<F>)>,
    pub entry: usize,
}
pub type Result<T> = core::result::Result<T, &'static str>;

impl<F: Frame> Image<F> {
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
            let legacy = start >= BASE && end <= LIMIT;
            let high =
                machine == elf::EM_X86_64 && address >= HIGH_USER_START && end as u64 <= USER_END;
            if start < prior_end || !(legacy || high) {
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
                    m.frame
                        .write(first - base, &contents[first - start..last - start]);
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
        // Keep binary searches valid when the ABI stack precedes high segments.
        image.pages.sort_unstable_by_key(|page| page.0);
        Ok(image)
    }

    // PT_LOAD segments are already checked for monotonically increasing addresses.
    // Adjacent segments may share their boundary page.
    fn append_page(&mut self, addr: usize) -> Result<&mut Mapping<F>> {
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
    pub fn pages(&self) -> impl Iterator<Item = (usize, &Mapping<F>)> {
        self.pages.iter().map(|(addr, mapping)| (*addr, mapping))
    }
    pub fn pages_mut(&mut self) -> impl Iterator<Item = (usize, &mut Mapping<F>)> {
        self.pages
            .iter_mut()
            .map(|(addr, mapping)| (*addr, mapping))
    }
    pub fn check(&self, addr: usize, len: usize, write: bool) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let end = addr.checked_add(len).ok_or("invalid user address")?;
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
            self.pages[self.index(addr - offset)?]
                .1
                .frame
                .read(offset, &mut out[..n]);
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
            self.pages[index].1.frame.write(offset, &input[..n]);
            addr += n;
            input = &input[n..];
        }
        Ok(())
    }

    /// Allocate the first suitably sized hole. Keep existing mappings untouched
    /// even if the backing store runs out partway through the request.
    pub fn map_anonymous(&mut self, count: usize) -> Result<usize> {
        if count == 0 {
            return Err("invalid anonymous mapping");
        }
        let size = count
            .checked_mul(PAGE)
            .ok_or("insufficient virtual memory")?;
        let mut base = ANON_BASE;
        for &(address, _) in &self.pages {
            if address < base {
                continue;
            }
            if address >= ANON_END || size <= address - base {
                break;
            }
            base = address + PAGE;
        }
        if size > ANON_END - base {
            return Err("insufficient virtual memory");
        }
        if count > F::available() {
            return Err("insufficient physical memory");
        }
        self.pages
            .try_reserve(count)
            .map_err(|_| "insufficient page metadata")?;
        let start = self.pages.len();
        for address in (base..base + size).step_by(PAGE) {
            match Mapping::zero() {
                Ok(mut mapping) => {
                    mapping.writable = true;
                    self.pages.push((address, mapping));
                }
                Err(error) => {
                    self.pages.truncate(start);
                    return Err(error);
                }
            }
        }
        // Move the appended run into address order without a second allocation.
        let insert = self.pages[..start].partition_point(|page| page.0 < base);
        self.pages[insert..].rotate_right(count);
        Ok(base)
    }

    fn unmap_range(&self, base: usize, count: usize) -> Result<core::ops::Range<usize>> {
        if count == 0
            || base % PAGE != 0
            || !(ANON_BASE..ANON_END).contains(&base)
            || count > (ANON_END - base) / PAGE
        {
            return Err("invalid anonymous mapping");
        }
        let i = self.index(base)?;
        let end = i.checked_add(count).ok_or("invalid anonymous mapping")?;
        if self.pages.get(end - 1).map(|p| p.0) != Some(base + (count - 1) * PAGE) {
            return Err("unmapped user address");
        }
        Ok(i..end)
    }

    pub fn validate_unmap(&self, base: usize, count: usize) -> Result<()> {
        self.unmap_range(base, count).map(|_| ())
    }

    /// Caller has retired all hardware page tables that still name these frames.
    pub fn unmap_anonymous(&mut self, base: usize, count: usize) -> Result<()> {
        let range = self.unmap_range(base, count)?;
        self.pages.drain(range);
        Ok(())
    }
}
impl<F: Frame> Mapping<F> {
    fn zero() -> Result<Self> {
        Ok(Self {
            frame: F::allocate_zeroed()?,
            writable: false,
            executable: false,
        })
    }
    pub fn frame_address(&self) -> usize {
        self.frame.frame_address()
    }
}
