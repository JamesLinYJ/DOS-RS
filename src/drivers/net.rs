//! Ethernet service. Packet protocols depend only on Link, not PCI registers.
mod e1000;
use crate::{
    kernel_cell::KernelCell,
    platform::{hex, pci::Device},
};
use core::net::Ipv4Addr;
pub(super) trait Link {
    fn mac(&self) -> [u8; 6];
    fn send(&mut self, frame: &[u8]) -> bool;
    fn receive(&mut self, frame: &mut [u8]) -> Option<usize>;
}
static NIC: KernelCell<Option<e1000::Nic>> = KernelCell::new(None);
pub(super) fn matches(d: Device) -> bool {
    d.vendor == 0x8086 && d.id == 0x100e && d.class >> 8 == 0x0200
}
pub(super) fn attach(d: Device) -> Result<(), &'static str> {
    let mut nic = NIC.borrow();
    if nic.is_some() {
        return Err("additional Ethernet controller");
    }
    *nic = Some(e1000::Nic::new(d)?);
    Ok(())
}
fn print_mac(bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        if i != 0 {
            crate::putc(b':')
        }
        hex(u32::from(*b), 2);
    }
}
pub(super) fn command(args: &[u8]) {
    let mut nic = NIC.borrow();
    let Some(nic) = nic.as_mut() else {
        crate::puts("Ethernet unavailable.\n");
        return;
    };
    if args.is_empty() {
        crate::puts("Ethernet ");
        print_mac(&nic.mac());
        crate::puts("\n");
        return;
    }
    let mut words = args
        .split(|b| b.is_ascii_whitespace())
        .filter(|s| !s.is_empty());
    let op = words.next().unwrap_or_default();
    let address =
        |value: Option<&[u8]>| core::str::from_utf8(value?).ok()?.parse::<Ipv4Addr>().ok();
    let (Some(local), Some(peer)) = (address(words.next()), address(words.next())) else {
        crate::puts("NET ARP <local-ip> <peer-ip>\n");
        return;
    };
    if !op.eq_ignore_ascii_case(b"ARP") || words.next().is_some() {
        crate::puts("NET ARP <local-ip> <peer-ip>\n");
        return;
    }
    if let Some(mac) = resolve(nic, local.octets(), peer.octets()) {
        crate::puts("ARP reply ");
        print_mac(&mac);
        crate::puts("\n");
    } else {
        crate::puts("No ARP reply.\n");
    }
}
fn resolve(link: &mut impl Link, local: [u8; 4], peer: [u8; 4]) -> Option<[u8; 6]> {
    let mac = link.mac();
    let mut tx = [0u8; 60];
    tx[..6].fill(255);
    tx[6..12].copy_from_slice(&mac);
    tx[12..22].copy_from_slice(&[8, 6, 0, 1, 8, 0, 6, 4, 0, 1]);
    tx[22..28].copy_from_slice(&mac);
    tx[28..32].copy_from_slice(&local);
    tx[38..42].copy_from_slice(&peer);
    if !link.send(&tx) {
        return None;
    }
    let mut rx = [0u8; 1518];
    for _ in 0..200 {
        if let Some(n) = link.receive(&mut rx)
            && n >= 42
            && rx[..6] == mac
            && rx[12..22] == [8, 6, 0, 1, 8, 0, 6, 4, 0, 2]
            && rx[28..32] == peer
            && rx[32..38] == mac
            && rx[38..42] == local
        {
            return rx[22..28].try_into().ok();
        }
        if !crate::arch::platform::pit_wait(11932) {
            return None;
        }
    }
    None
}
