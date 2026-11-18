//! VirtIO PCI configuration-space access to modern common/notify capabilities.
//! This also reaches 64-bit memory BARs from the 32-bit kernel.
use crate::platform::pci::Device;

#[derive(Clone, Copy)]
struct Region {
    bar: u8,
    offset: u32,
    length: u32,
}
impl Region {
    fn address(self, offset: u32, size: u8) -> Option<u32> {
        if offset.checked_add(u32::from(size))? > self.length {
            return None;
        }
        self.offset.checked_add(offset)
    }
}

pub(super) struct Transport {
    device: Device,
    access: u8,
    common: Region,
    notify: Region,
    multiplier: u32,
    queue_notify: u32,
}

impl Transport {
    pub fn new(device: Device) -> Option<Self> {
        let mut next = device.read(0x34) as u8 & !3;
        let (mut common, mut notify, mut access) = (None, None, None);
        let mut multiplier = 0;
        // Conventional PCI capability offsets are DWORD aligned in [0x40,0xfc].
        for _ in 0..48 {
            if !(0x40..=0xfc).contains(&next) {
                break;
            }
            let header = device.read(next);
            let len = (header >> 16) as u8;
            if header as u8 == 9 && len >= 16 && usize::from(next) + usize::from(len) <= 256 {
                let kind = header >> 24;
                let region = Region {
                    bar: device.read(next + 4) as u8,
                    offset: device.read(next + 8),
                    length: device.read(next + 12),
                };
                match kind {
                    1 if region.bar < 6 && region.length >= 56 => common = Some(region),
                    2 if region.bar < 6 && len >= 20 => {
                        notify = Some(region);
                        multiplier = device.read(next + 16);
                    }
                    5 if len >= 20 => access = Some(next),
                    _ => {}
                }
            }
            next = (header >> 8) as u8 & !3;
        }
        Some(Self {
            device,
            common: common?,
            notify: notify?,
            access: access?,
            multiplier,
            queue_notify: 0,
        })
    }

    fn select(&self, region: Region, offset: u32, size: u8) -> Option<u8> {
        let address = region.address(offset, size)?;
        self.device
            .write_config(self.access + 4, 1, u32::from(region.bar));
        self.device.write(self.access + 8, address);
        self.device.write(self.access + 12, u32::from(size));
        Some(self.access + 16)
    }

    fn read(&self, offset: u32, size: u8) -> u32 {
        let window = self
            .select(self.common, offset, size)
            .expect("validated common configuration size");
        self.device.read_config(window, size)
    }
    fn write(&self, offset: u32, size: u8, value: u32) {
        let window = self
            .select(self.common, offset, size)
            .expect("validated common configuration size");
        self.device.write_config(window, size, value);
    }
    pub fn initialize(&mut self, desc: u32, avail: u32, used: u32) -> Option<()> {
        self.device.enable(6);
        self.write(20, 1, 0);
        if !(0..10_000).any(|_| self.read(20, 1) == 0) {
            return None;
        }
        self.write(20, 1, 1);
        self.write(20, 1, 3);
        self.write(0, 4, 1);
        if self.read(4, 4) & 1 == 0 {
            return None;
        }
        self.write(8, 4, 0);
        self.write(12, 4, 0);
        self.write(8, 4, 1);
        self.write(12, 4, 1); // VIRTIO_F_VERSION_1
        self.write(20, 1, 11);
        if self.read(20, 1) & 8 == 0 {
            return None;
        }
        self.write(22, 2, 0);
        if self.read(24, 2) == 0 {
            return None;
        }
        self.write(24, 2, 1); // Single descriptor and a single in-flight request.
        self.queue_notify = self.read(30, 2).checked_mul(self.multiplier)?;
        self.notify.address(self.queue_notify, 2)?;
        for (reg, addr) in [(32, desc), (40, avail), (48, used)] {
            self.write(reg, 4, addr);
            self.write(reg + 4, 4, 0);
        }
        self.write(28, 2, 1);
        self.write(20, 1, 15);
        Some(())
    }
    pub fn notify(&self) {
        let window = self
            .select(self.notify, self.queue_notify, 2)
            .expect("validated queue offset");
        self.device.write_config(window, 2, 0);
    }
}
