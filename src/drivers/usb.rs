//! USB device enumeration and class matching, independent of the host controller.
mod hci;
mod storage;
mod xhci;
use crate::{
    kernel_cell::KernelCell,
    platform::{hex, number, pci::Device},
};
#[derive(Clone, Copy, Debug)]
pub(crate) struct Error(pub &'static str);
type Result<T> = core::result::Result<T, Error>;
#[derive(Clone, Copy, Default)]
struct Endpoint {
    address: u8,
    kind: u8,
    packet: u16,
    interval: u8,
}
impl Endpoint {
    fn dci(self) -> u8 {
        if self.address == 0 {
            1
        } else {
            (self.address & 15) * 2 + u8::from(self.address & 128 != 0)
        }
    }
}
#[derive(Clone, Copy)]
struct Setup {
    kind: u8,
    request: u8,
    value: u16,
    index: u16,
    len: u16,
}
impl Setup {
    fn bits(self) -> u64 {
        u64::from(self.kind)
            | (u64::from(self.request) << 8)
            | (u64::from(self.value) << 16)
            | (u64::from(self.index) << 32)
            | (u64::from(self.len) << 48)
    }
}
trait UsbIo {
    fn control(&mut self, slot: u8, setup: Setup, bytes: &mut [u8]) -> Result<usize>;
    fn transfer(&mut self, slot: u8, ep: Endpoint, bytes: &mut [u8]) -> Result<usize>;
}
#[derive(Clone, Copy, Default)]
struct Interface {
    number: u8,
    class: [u8; 3],
    ep: [Endpoint; 8],
    count: usize,
}
impl Interface {
    fn endpoint(self, kind: u8, input: bool) -> Result<Endpoint> {
        self.ep[..self.count]
            .iter()
            .copied()
            .find(|e| e.kind == kind && (e.address & 128 != 0) == input)
            .ok_or(Error("missing class endpoint"))
    }
}
#[derive(Clone, Copy)]
struct DeviceInfo {
    slot: u8,
    vendor: u16,
    product: u16,
    speed: u8,
    interfaces: [Interface; 8],
    count: usize,
}
trait HostController: UsbIo {
    fn packet_size(&mut self, id: u8, packet: u16) -> Result<()>;
    fn configure(&mut self, id: u8, ep: Endpoint) -> Result<()>;
}
const CLASS_DRIVERS: &[fn(&Interface) -> bool] = &[storage::matches, hci::matches];
struct Host {
    controller: xhci::Controller,
    devices: [Option<DeviceInfo>; 8],
}
static HOSTS: KernelCell<[Option<Host>; 4]> = KernelCell::new([const { None }; 4]);
pub(super) fn matches(d: Device) -> bool {
    d.class == 0x0c0330
}
pub(super) fn attach(d: Device) -> core::result::Result<(), &'static str> {
    let mut hosts = HOSTS.borrow();
    let entry = hosts
        .iter_mut()
        .find(|h| h.is_none())
        .ok_or("USB host limit")?;
    let mut hc = xhci::Controller::new(d).map_err(|e| e.0)?;
    let mut devices = [None; 8];
    for port in 1..=hc.ports() {
        let result = (|| {
            let Some((slot, speed)) = hc.address(port)? else {
                return Ok(());
            };
            let dev = enumerate(&mut hc, slot, speed)?;
            devices[usize::from(slot) - 1] = Some(dev);
            Ok(())
        })();
        if let Err(Error(message)) = result {
            crate::puts("USB port ");
            number(port.into());
            crate::puts("  ");
            crate::puts(message);
            crate::puts("\n");
        }
    }
    *entry = Some(Host {
        controller: hc,
        devices,
    });
    Ok(())
}
fn descriptor(io: &mut impl UsbIo, slot: u8, kind: u8, out: &mut [u8]) -> Result<()> {
    let setup = Setup {
        kind: 0x80,
        request: 6,
        value: u16::from(kind) << 8,
        index: 0,
        len: out.len() as u16,
    };
    if io.control(slot, setup, out)? != out.len() {
        return Err(Error("short USB descriptor"));
    }
    Ok(())
}
fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn enumerate(hc: &mut impl HostController, slot: u8, speed: u8) -> Result<DeviceInfo> {
    let mut device = [0; 18];
    descriptor(hc, slot, 1, &mut device[..8])?;
    let packet = if speed >= 4 {
        512
    } else {
        u16::from(device[7])
    };
    if !matches!(packet, 8 | 16 | 32 | 64 | 512) {
        return Err(Error("invalid EP0 packet"));
    }
    hc.packet_size(slot, packet)?;
    descriptor(hc, slot, 1, &mut device)?;
    let mut config = [0; 1024];
    descriptor(hc, slot, 2, &mut config[..9])?;
    let len = usize::from(word(&config, 2));
    if !(9..=config.len()).contains(&len) {
        return Err(Error("configuration too large"));
    }
    descriptor(hc, slot, 2, &mut config[..len])?;
    let mut dev = DeviceInfo {
        slot,
        vendor: word(&device, 8),
        product: word(&device, 10),
        speed,
        interfaces: [Interface::default(); 8],
        count: 0,
    };
    let mut descriptors = &config[9..len];
    let mut current = None;
    while !descriptors.is_empty() {
        let n = usize::from(descriptors[0]);
        if n < 2 || n > descriptors.len() {
            return Err(Error("invalid USB descriptor"));
        }
        let d = &descriptors[..n];
        descriptors = &descriptors[n..];
        match d[1] {
            4 if n >= 9 => {
                current = None;
                if d[3] == 0 && dev.count < dev.interfaces.len() {
                    current = Some(dev.count);
                    dev.interfaces[dev.count] = Interface {
                        number: d[2],
                        class: [d[5], d[6], d[7]],
                        ..Interface::default()
                    };
                    dev.count += 1;
                }
            }
            5 if n >= 7 => {
                if let Some(i) = current {
                    let iface = &mut dev.interfaces[i];
                    if iface.count < iface.ep.len() && d[2] & 15 != 0 {
                        iface.ep[iface.count] = Endpoint {
                            address: d[2],
                            kind: d[3] & 3,
                            packet: word(d, 4) & 2047,
                            interval: d[6],
                        };
                        iface.count += 1;
                    }
                }
            }
            _ => {}
        }
    }
    for iface in &dev.interfaces[..dev.count] {
        if CLASS_DRIVERS.iter().any(|matches| matches(iface)) {
            for &ep in &iface.ep[..iface.count] {
                if matches!(ep.kind, 2 | 3) {
                    hc.configure(slot, ep)?;
                }
            }
        }
    }
    hc.control(
        slot,
        Setup {
            kind: 0,
            request: 9,
            value: u16::from(config[5]),
            index: 0,
            len: 0,
        },
        &mut [],
    )?;
    Ok(dev)
}
fn print_error(e: Error) {
    crate::puts(e.0);
    crate::puts("\n");
}
pub(super) fn command(args: &[u8]) {
    let read = if let Some((verb, tail)) = args.split_first_chunk::<4>()
        && verb.eq_ignore_ascii_case(b"READ")
    {
        let Some(lba) = core::str::from_utf8(tail.trim_ascii())
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            crate::puts("USB READ <sector>\n");
            return;
        };
        Some(lba)
    } else if args.is_empty() {
        None
    } else {
        crate::puts("USB or USB READ <sector>\n");
        return;
    };
    let mut hosts = HOSTS.borrow();
    let mut found = false;
    for host in hosts.iter_mut().flatten() {
        for dev in host.devices.iter().flatten() {
            if read.is_none() {
                found = true;
                crate::puts("USB ");
                hex(dev.vendor.into(), 4);
                crate::putc(b':');
                hex(dev.product.into(), 4);
                crate::puts(match dev.speed {
                    3 => "  high-speed (480 Mbps)",
                    4 => "  SuperSpeed",
                    1 => "  full-speed",
                    2 => "  low-speed",
                    _ => "  USB",
                });
                crate::puts("\n");
            }
            for iface in &dev.interfaces[..dev.count] {
                if let Some(lba) = read
                    && storage::matches(iface)
                {
                    let mut sector = [0; 512];
                    match storage::read(&mut host.controller, dev.slot, *iface, lba, &mut sector) {
                        Ok(()) => {
                            crate::puts("USB sector ");
                            number(lba);
                            crate::puts("  ");
                            for b in &sector[..32] {
                                hex(u32::from(*b), 2)
                            }
                            crate::puts("\n");
                        }
                        Err(e) => print_error(e),
                    }
                    return;
                }
            }
        }
    }
    if !found {
        crate::puts("No matching USB device.\n");
    }
}
pub(super) fn bluetooth(args: &[u8]) {
    let mut hosts = HOSTS.borrow();
    for host in hosts.iter_mut().flatten() {
        for dev in host.devices.iter().flatten() {
            for &iface in &dev.interfaces[..dev.count] {
                if hci::matches(&iface) {
                    if let Err(e) = hci::command(&mut host.controller, dev.slot, iface, args) {
                        print_error(e);
                    }
                    return;
                }
            }
        }
    }
    crate::puts("USB Bluetooth controller unavailable.\n");
}
