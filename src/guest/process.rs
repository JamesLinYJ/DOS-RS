//! Common COM/MZ loader used by boot, the resident shell and DOS EXEC.
use super::arena::Arena;
use super::memory::{GuestRam, read_word};
use super::mz::Mz;
use super::state::{CpuState, Slot};

pub(super) struct CommandTail {
    bytes: [u8; 126],
    len: usize,
}

impl CommandTail {
    pub fn empty() -> Self {
        Self {
            bytes: [0; 126],
            len: 0,
        }
    }

    pub fn arguments(args: &[u8]) -> Self {
        let mut tail = Self::empty();
        if !args.is_empty() {
            tail.bytes[0] = b' ';
            tail.len = 1 + args.len().min(125);
            tail.bytes[1..tail.len].copy_from_slice(&args[..tail.len - 1]);
        }
        tail
    }

    pub fn from_exec(ram: &GuestRam, segment: u16, offset: u16) -> Self {
        let address = read_word(ram, segment, offset.wrapping_add(2)).unwrap_or(0);
        let selector = read_word(ram, segment, offset.wrapping_add(4)).unwrap_or(0);
        let mut tail = Self::empty();
        if selector != 0 || address != 0 {
            tail.len = usize::from(ram.load(selector, address).unwrap_or(0)).min(tail.bytes.len());
            for (index, byte) in tail.bytes[..tail.len].iter_mut().enumerate() {
                *byte = ram
                    .load(selector, address.wrapping_add(index as u16 + 1))
                    .unwrap_or(0);
            }
        }
        tail
    }
}

pub(super) struct Program<'a> {
    pub name: &'a [u8; 11],
    /// Absolute DOS directory prefix, including its trailing separator.
    pub directory: &'a [u8],
    pub image: &'a [u8],
    pub preferred_psp: u16,
    /// Initial paragraphs for a flat COM image; MZ images supply their own bounds.
    pub com_paragraphs: u16,
    pub environment: u16,
    pub parent: u16,
    pub tail: CommandTail,
}

impl Program<'_> {
    pub fn load(self, ram: &mut GuestRam, arena: &mut Arena) -> Result<(u16, CpuState), u16> {
        let mz = if self.image.starts_with(b"MZ") {
            Some(Mz::parse(self.image).ok_or(11u16)?)
        } else {
            None
        };
        let (min, max) = match &mz {
            Some(mz) => (mz.minimum, mz.maximum),
            None => {
                if self.image.len() > 0xFEFE || self.image.is_empty() {
                    return Err(11);
                }
                let size = (0x102 + self.image.len()).div_ceil(16) as u16;
                let size = size.max(self.com_paragraphs);
                // WIN.COM checks PSP:0002 before it can call DOS to resize.
                // Identify the Windows launcher from its header so a renamed
                // copy receives the same conventional-memory grant.
                let windows_launcher = self.image.get(3..5) == Some(b"XW")
                    && self.image.get(19..27) == Some(b"SMARTAAR");
                (size, if windows_launcher { u16::MAX } else { size })
            }
        };

        let default_env = b"PATH=\\\0\0";
        let env_len = if self.environment == 0 {
            default_env.len()
        } else {
            (1..0x8000u16)
                .find(|&i| read_word(ram, self.environment, i - 1) == Some(0))
                .map(|i| usize::from(i) + 1)
                .ok_or(10u16)?
        };
        // Two terminators, a word count, and the full ASCIIZ program name.
        let base = self.name[..8].iter().position(|&b| b == b' ').unwrap_or(8);
        let ext = self.name[8..].iter().position(|&b| b == b' ').unwrap_or(3);
        let env_size = (env_len + 2 + self.directory.len() + base + usize::from(ext != 0) + ext + 1)
            .div_ceil(16) as u16;
        let (psp, end) = arena
            .process(self.preferred_psp, env_size, min, max)
            .map_err(|_| 8u16)?;
        let env = psp - env_size - 1;
        let write_image = || -> Option<CpuState> {
            for i in 0..env_len {
                let byte = if self.environment == 0 {
                    default_env[i]
                } else {
                    ram.load(self.environment, i as u16)?
                };
                if !ram.store(env, i as u16, byte) {
                    return None;
                }
            }
            // DOS directory prefix (68 bytes), file name (12), trailer and paragraph padding.
            let mut storage = [0u8; 100];
            let trailer = storage.get_mut(..usize::from(env_size) * 16 - env_len)?;
            trailer[..2].copy_from_slice(&1u16.to_le_bytes());
            let file = 2 + self.directory.len();
            trailer[2..file].copy_from_slice(self.directory);
            trailer[file..file + base].copy_from_slice(&self.name[..base]);
            if ext > 0 {
                trailer[file + base] = b'.';
                trailer[file + base + 1..file + base + 1 + ext]
                    .copy_from_slice(&self.name[8..8 + ext]);
            }
            let mut block = [0; 256];
            if !ram.write_dos(u32::from(env) * 16 + env_len as u32, trailer) {
                return None;
            }
            let base = u32::from(psp) * 16;
            let size = usize::from(end - psp) * 16;
            for at in (0..size).step_by(4096) {
                if !ram.write_dos(base + at as u32, &[0; 4096][..(size - at).min(4096)]) {
                    return None;
                }
            }
            block[..2].copy_from_slice(&[0xCD, 0x20]);
            block[2..4].copy_from_slice(&end.to_le_bytes());
            block[0x16..0x18].copy_from_slice(&self.parent.to_le_bytes());
            block[0x2C..0x2E].copy_from_slice(&env.to_le_bytes());
            block[0x80] = self.tail.len as u8;
            block[0x81..0x81 + self.tail.len].copy_from_slice(&self.tail.bytes[..self.tail.len]);
            block[0x81 + self.tail.len] = b'\r';
            if !ram.write_dos(base, &block) {
                return None;
            }
            let cpu = match mz {
                Some(mz) => mz.load(ram, psp, end)?,
                None => {
                    if !ram.write_dos(base + 0x100, self.image) {
                        return None;
                    }
                    let mut cpu = CpuState::at_com(psp);
                    cpu.set_wide(Slot::Stack, (size.min(0x10000) - 2) as u16);
                    cpu
                }
            };
            Some(cpu)
        };
        let Some(cpu) = write_image() else {
            arena.release_process(psp);
            return Err(8);
        };
        arena.publish(ram);
        Ok((psp, cpu))
    }
}
