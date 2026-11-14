//! A process owns its DOS handle numbers. Aliases share one open description.
use super::{GuestRam, HANDLE_SLOTS, Opened, console_handle, flush_opened};

const UNUSED: u8 = 0xff;
const SYSTEM_FILES: usize = 40;
pub(super) const SFT_SEGMENT: u16 = 0x00a0;
const SFT_ENTRY: usize = 0x3b;

#[derive(Clone, Copy)]
pub(super) struct Job {
    map: [u8; HANDLE_SLOTS],
    pub limit: u16,
}
#[derive(Clone, Copy)]
struct Description {
    file: Opened,
    refs: u16,
    inherit: bool,
}

pub(super) struct Handles {
    pub job: Job,
    descriptions: [Option<Description>; SYSTEM_FILES],
}

impl Handles {
    pub fn new() -> Self {
        let mut table = Self {
            job: Job {
                map: [UNUSED; HANDLE_SLOTS],
                limit: HANDLE_SLOTS as u16,
            },
            descriptions: [None; SYSTEM_FILES],
        };
        for h in 0..3 {
            table.insert(h, h, console_handle(h as u16), true);
        }
        table
    }
    pub fn live(&self, slot: usize) -> bool {
        self.descriptions[slot].is_some()
    }
    pub fn slot(&self, h: u16) -> Option<usize> {
        let slot = *self.job.map.get(usize::from(h))?;
        (slot != UNUSED).then_some(usize::from(slot))
    }
    pub fn get(&self, h: u16) -> Option<&Opened> {
        Some(&self.descriptions[self.slot(h)?].as_ref()?.file)
    }
    pub fn get_mut(&mut self, h: u16) -> Option<&mut Opened> {
        let slot = self.slot(h)?;
        Some(&mut self.descriptions[slot].as_mut()?.file)
    }
    pub fn available(&self) -> Option<(usize, usize)> {
        let h = self.job.map[..usize::from(self.job.limit)]
            .iter()
            .position(|&v| v == UNUSED)?;
        Some((h, self.descriptions.iter().position(Option::is_none)?))
    }
    pub fn insert(&mut self, h: usize, slot: usize, file: Opened, inherit: bool) {
        self.descriptions[slot] = Some(Description {
            file,
            refs: 1,
            inherit,
        });
        self.job.map[h] = slot as u8;
    }
    pub fn span(&self) -> u16 {
        self.job
            .map
            .iter()
            .rposition(|&n| n != UNUSED)
            .map_or(0, |n| n as u16 + 1)
            .max(3)
    }
    pub fn close(&mut self, h: u16) -> Result<(), u16> {
        let slot = self.slot(h).ok_or(6u16)?;
        let entry = self.descriptions[slot].as_mut().unwrap();
        if entry.refs == 1 && !flush_opened(&entry.file) {
            return Err(5);
        }
        entry.refs -= 1;
        if entry.refs == 0 {
            self.descriptions[slot] = None;
        }
        self.job.map[h as usize] = UNUSED;
        Ok(())
    }
    pub fn duplicate(&mut self, source: u16, target: Option<u16>) -> Result<u16, u16> {
        let slot = self.slot(source).ok_or(6u16)?;
        let h = match target {
            Some(h) if h < self.job.limit => h,
            Some(_) => return Err(6),
            None => self.job.map[..self.job.limit as usize]
                .iter()
                .position(|&v| v == UNUSED)
                .ok_or(4u16)? as u16,
        };
        if self.slot(h) == Some(slot) {
            return Ok(h);
        }
        if self.slot(h).is_some() {
            self.close(h)?;
        }
        self.descriptions[slot].as_mut().unwrap().refs += 1;
        self.job.map[h as usize] = slot as u8;
        Ok(h)
    }
    pub fn inherit(&mut self) -> Job {
        let parent = self.job;
        for entry in &mut self.job.map {
            if *entry == UNUSED {
                continue;
            }
            let desc = self.descriptions[*entry as usize].as_mut().unwrap();
            if desc.inherit {
                desc.refs += 1;
            } else {
                *entry = UNUSED;
            }
        }
        parent
    }
    pub fn leave(&mut self, parent: Job) -> bool {
        let mut ok = true;
        for h in 0..self.job.limit {
            if let Some(slot) = self.slot(h)
                && self.close(h).is_err()
            {
                // A terminating job cannot retry. Release its final reference and report the failure.
                self.descriptions[slot] = None;
                ok = false;
            }
        }
        self.job = parent;
        ok
    }
    pub fn publish_sft(&self, ram: &mut GuestRam) {
        let mut table = [0; 6 + SYSTEM_FILES * SFT_ENTRY];
        table[..4].fill(0xff);
        table[4..6].copy_from_slice(&(SYSTEM_FILES as u16).to_le_bytes());
        for (entry, desc) in table[6..]
            .chunks_exact_mut(SFT_ENTRY)
            .zip(&self.descriptions)
        {
            entry.fill(0);
            let Some(desc) = desc else {
                continue;
            };
            entry[..2].copy_from_slice(&desc.refs.to_le_bytes());
            entry[2] = desc.file.access;
            entry[5..7].copy_from_slice(
                &(if desc.file.named {
                    if super::super::disk::root(desc.file.dir) != 0 {
                        2u16
                    } else {
                        0
                    }
                } else {
                    0x80
                })
                .to_le_bytes(),
            );
            entry[0x15..0x19].copy_from_slice(&(desc.file.pos as u32).to_le_bytes());
            entry[0x20..0x2b].copy_from_slice(&desc.file.name);
        }
        ram.write_dos(u32::from(SFT_SEGMENT) * 16, &table);
    }
    pub fn publish(&self, ram: &mut GuestRam, psp: u16) {
        self.publish_sft(ram);
        let mut page = [0; 0x38];
        page[0x18..0x18 + HANDLE_SLOTS].copy_from_slice(&self.job.map);
        page[0x32..0x34].copy_from_slice(&self.job.limit.to_le_bytes());
        page[0x34..0x36].copy_from_slice(&0x18u16.to_le_bytes());
        page[0x36..0x38].copy_from_slice(&psp.to_le_bytes());
        let base = u32::from(psp) * 16;
        ram.write_dos(base + 0x18, &page[0x18..0x18 + HANDLE_SLOTS]);
        ram.write_dos(base + 0x32, &page[0x32..]);
    }
    pub fn commit_all(&mut self) -> bool {
        self.descriptions.iter_mut().flatten().all(|desc| {
            if !flush_opened(&desc.file) {
                return false;
            }
            desc.file.dirty = false;
            true
        })
    }
}
