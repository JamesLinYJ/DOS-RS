//! QEMU standard VGA linear framebuffer for the legacy BIOS boot path.
use crate::{
    arch::{inw, outw},
    bios::{pc, screen},
    platform::pci::Device,
};

pub(super) fn matches(device: Device) -> bool {
    device.vendor == 0x1234 && device.id == 0x1111 && device.class >> 8 == 0x0300
}
pub(super) fn attach(device: Device) -> Result<(), &'static str> {
    if screen::supports_graphics() {
        return Ok(());
    }
    let fb = device
        .mmio()
        .filter(|bar| bar.fits(0, 640 * 480 * 4))
        .ok_or("Missing VGA framebuffer")?;
    device.enable(3);
    outw(0x1ce, 0);
    if !(0xb0c0..=0xb0c5).contains(&inw(0x1cf)) {
        return Err("Unsupported VGA mode interface");
    }
    for (index, value) in [(4, 0), (1, 640), (2, 480), (3, 32), (4, 0x41)] {
        outw(0x1ce, index);
        outw(0x1cf, value);
    }
    let (base, len) = fb.aperture();
    screen::initialize(base, len, 640, 480, 640, false);
    pc::restore_text();
    Ok(())
}
