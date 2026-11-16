//! Byte-addressed access to the primary ATA disk for native volume drivers.
use super::ata;
use dosrs_block::{BlockDevice, BlockRead, Error};

pub(crate) struct AtaVolume {
    sectors: u32,
}
impl AtaVolume {
    pub fn primary() -> Option<Self> {
        Some(Self {
            sectors: ata::capacity()?,
        })
    }
    fn check(&self, offset: u64, len: usize) -> Result<(), Error> {
        if offset
            .checked_add(len as u64)
            .is_some_and(|end| end <= self.size_bytes())
        {
            Ok(())
        } else {
            Err(Error)
        }
    }
}
impl BlockRead for AtaVolume {
    fn size_bytes(&self) -> u64 {
        u64::from(self.sectors) * 512
    }
    fn read_at(&self, mut offset: u64, mut out: &mut [u8]) -> Result<(), Error> {
        self.check(offset, out.len())?;
        while !out.is_empty() {
            let lba = (offset / 512) as u32;
            let within = (offset % 512) as usize;
            let n = if within == 0 && out.len() >= 512 {
                let n = out.len() / 512 * 512;
                if !ata::read_sectors(lba, &mut out[..n]) {
                    return Err(Error);
                }
                n
            } else {
                let mut sector = [0; 512];
                if !ata::read(lba, &mut sector) {
                    return Err(Error);
                }
                let n = (512 - within).min(out.len());
                out[..n].copy_from_slice(&sector[within..within + n]);
                n
            };
            out = &mut out[n..];
            offset += n as u64;
        }
        Ok(())
    }
}
impl BlockDevice for AtaVolume {
    fn is_writable(&self) -> bool {
        true
    }
    fn write_at(&self, mut offset: u64, mut data: &[u8]) -> Result<(), Error> {
        self.check(offset, data.len())?;
        while !data.is_empty() {
            let lba = (offset / 512) as u32;
            let within = (offset % 512) as usize;
            let n = (512 - within).min(data.len());
            let mut sector = [0; 512];
            if n != 512 && !ata::read(lba, &mut sector) {
                return Err(Error);
            }
            sector[within..within + n].copy_from_slice(&data[..n]);
            if !ata::write(lba, &sector) {
                return Err(Error);
            }
            offset += n as u64;
            data = &data[n..];
        }
        Ok(())
    }
    fn flush(&self) -> Result<(), Error> {
        ata::flush().then_some(()).ok_or(Error)
    }
}
