//! PCI configuration mechanism 1. Follow firmware-configured bridges from bus zero.
use super::{hex, number};
use crate::arch::platform::{port32, put32};
#[derive(Clone, Copy, Default)]
pub(crate) struct Device {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor: u16,
    pub id: u16,
    pub class: u32,
    pub header: u8,
}
impl Device {
    fn select(self, reg: u8) {
        put32(
            0xcf8,
            0x80000000
                | u32::from(self.bus) << 16
                | u32::from(self.slot) << 11
                | u32::from(self.function) << 8
                | u32::from(reg & !3),
        );
    }
    pub fn read(self, reg: u8) -> u32 {
        self.select(reg);
        port32(0xcfc)
    }
    pub(crate) fn enable(self, bits: u16) {
        self.select(4);
        let old = crate::arch::inw(0xcfc);
        crate::arch::outw(0xcfc, old | bits);
    }
    pub(crate) fn write(self, reg: u8, v: u32) {
        self.select(reg);
        put32(0xcfc, v);
    }
    pub(crate) fn mmio(self) -> Option<crate::arch::io_memory::Registers> {
        if self.header & 0x7f != 0 {
            return None;
        }
        let low = self.read(0x10);
        if low & 1 != 0 || !matches!(low & 6, 0 | 4) {
            return None;
        }
        let wide = low & 4 != 0;
        let high = if wide { self.read(0x14) } else { 0 };
        let base = usize::try_from((u64::from(high) << 32) | u64::from(low & !15)).ok()?;
        if base == 0 {
            return None;
        }
        self.select(4);
        let command = crate::arch::inw(0xcfc);
        crate::arch::outw(0xcfc, command & !3);
        self.write(0x10, u32::MAX);
        if wide {
            self.write(0x14, u32::MAX);
        }
        let mask = self.read(0x10) & !15;
        let mask_hi = if wide { self.read(0x14) } else { u32::MAX };
        if wide {
            self.write(0x14, high);
        }
        self.write(0x10, low);
        self.select(4);
        crate::arch::outw(0xcfc, command);
        let size = (!((u64::from(mask_hi) << 32) | u64::from(mask))).checked_add(1)?;
        let len = usize::try_from(size).ok()?;
        if !len.is_power_of_two() {
            return None;
        }
        // BAR sizing completed while decoding was disabled. The firmware chose the address.
        unsafe { crate::arch::io_memory::Registers::from_bar(base, len) }
    }
    pub fn identity(self) {
        hex(self.bus.into(), 2);
        crate::putc(b':');
        hex(self.slot.into(), 2);
        crate::putc(b'.');
        number(self.function.into());
    }
}
pub(super) struct Bus {
    devices: [Device; 128],
    len: usize,
    overflow: usize,
}
impl Bus {
    pub const fn new() -> Self {
        Self {
            devices: [Device {
                bus: 0,
                slot: 0,
                function: 0,
                vendor: 0,
                id: 0,
                class: 0,
                header: 0,
            }; 128],
            len: 0,
            overflow: 0,
        }
    }
    pub fn scan(&mut self) {
        let mut buses = [0u8; 256];
        let mut seen = [false; 256];
        seen[0] = true;
        let mut next = 0;
        let mut tail = 1;
        while next < tail {
            let bus = buses[next];
            next += 1;
            for slot in 0..32 {
                for function in 0..8 {
                    let mut dev = Device {
                        bus,
                        slot,
                        function,
                        ..Device::default()
                    };
                    let id = dev.read(0);
                    if id as u16 == 0xffff {
                        if function == 0 {
                            break;
                        }
                        continue;
                    }
                    dev.vendor = id as u16;
                    dev.id = (id >> 16) as u16;
                    dev.class = dev.read(8) >> 8;
                    dev.header = (dev.read(12) >> 16) as u8;
                    if dev.class >> 8 == 0x0604 {
                        let child = (dev.read(0x18) >> 8) as u8;
                        if child != 0 && !seen[child as usize] {
                            seen[child as usize] = true;
                            buses[tail] = child;
                            tail += 1;
                        }
                    }
                    if self.len < self.devices.len() {
                        self.devices[self.len] = dev;
                        self.len += 1;
                    } else {
                        self.overflow += 1;
                    }
                    if function == 0 && dev.header & 0x80 == 0 {
                        break;
                    }
                }
            }
        }
    }
    pub fn devices(&self) -> &[Device] {
        &self.devices[..self.len]
    }
    pub fn report(&self) {
        for d in self.devices() {
            crate::puts("PCI ");
            d.identity();
            crate::puts("  ");
            hex(d.vendor.into(), 4);
            crate::putc(b':');
            hex(d.id.into(), 4);
            crate::puts(" class ");
            hex(d.class, 6);
            crate::puts("\n");
        }
        if self.overflow != 0 {
            crate::puts("Additional PCI functions: ");
            number(self.overflow as u32);
            crate::puts("\n");
        }
    }
}
