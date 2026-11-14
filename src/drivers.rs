//! Register a bus driver here; DOS and the scheduler consume device services.
#![forbid(unsafe_code)]
mod net;
mod usb;
mod wireless;
use crate::platform::pci::Device;

struct PciDriver {
    matches: fn(Device) -> bool,
    attach: fn(Device) -> Result<(), &'static str>,
}
const PCI_DRIVERS: &[PciDriver] = &[
    PciDriver {
        matches: usb::matches,
        attach: usb::attach,
    },
    PciDriver {
        matches: net::matches,
        attach: net::attach,
    },
    PciDriver {
        matches: wireless::matches,
        attach: wireless::attach,
    },
];
pub(crate) fn bind(devices: &[Device]) {
    for &dev in devices {
        if let Some(driver) = PCI_DRIVERS.iter().find(|d| (d.matches)(dev))
            && let Err(message) = (driver.attach)(dev)
        {
            crate::puts("Device ");
            dev.identity();
            crate::puts("  ");
            crate::puts(message);
            crate::puts("\n");
        }
    }
}
pub(crate) fn command(verb: &[u8], args: &[u8]) -> bool {
    for (name, handler) in [
        (b"USB".as_slice(), usb::command as fn(&[u8])),
        (b"BT", usb::bluetooth),
        (b"NET", net::command),
        (b"WIFI", wireless::command),
    ] {
        if verb.eq_ignore_ascii_case(name) {
            handler(args);
            return true;
        }
    }
    false
}
