//! Intel PCIe radio identification. Firmware requirements are explicit per family.
use crate::{
    arch::io_memory::Registers,
    kernel_cell::KernelCell,
    platform::{hex, pci::Device},
};
struct Family {
    id: u16,
    name: &'static str,
    firmware: &'static str,
}
const INTEL: &[Family] = &[
    Family {
        id: 0x24fd,
        name: "Wireless-AC 8265",
        firmware: "iwlwifi-8265-36.ucode",
    },
    Family {
        id: 0x2723,
        name: "Wi-Fi 6 AX200",
        firmware: "iwlwifi-cc-a0-77.ucode",
    },
    Family {
        id: 0x2725,
        name: "Wi-Fi 6E AX210",
        firmware: "iwlwifi-ty-a0-gf-a0-89.ucode",
    },
];
struct Radio {
    family: &'static Family,
    csr: Registers,
}
static RADIO: KernelCell<Option<Radio>> = KernelCell::new(None);
pub(super) fn matches(d: Device) -> bool {
    d.vendor == 0x8086 && d.class >> 8 == 0x0280 && INTEL.iter().any(|f| f.id == d.id)
}
pub(super) fn attach(d: Device) -> Result<(), &'static str> {
    let family = INTEL
        .iter()
        .find(|f| f.id == d.id)
        .ok_or("unknown Intel radio")?;
    let csr = d
        .mmio()
        .filter(|r| r.fits(0x28, 4))
        .ok_or("unmapped Intel radio CSR")?;
    d.enable(2);
    if csr.read(0x28) == u32::MAX {
        return Err("radio CSR unavailable");
    }
    let mut radio = RADIO.borrow();
    if radio.is_none() {
        *radio = Some(Radio { family, csr });
    }
    Ok(())
}
pub(super) fn command(_: &[u8]) {
    let radios = RADIO.borrow();
    let Some(r) = radios.as_ref() else {
        crate::puts("Intel wireless adapter unavailable.\n");
        return;
    };
    crate::puts(r.family.name);
    crate::puts("  revision ");
    hex(r.csr.read(0x28), 8);
    crate::puts(if r.csr.read(0x24) & (1 << 27) == 0 {
        "  RF disabled\n"
    } else {
        "  RF enabled\n"
    });
    crate::puts("Firmware family: ");
    crate::puts(r.family.firmware);
    crate::puts("\nRadio firmware startup pending.\n");
}
