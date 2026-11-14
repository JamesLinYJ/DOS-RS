//! Bluetooth USB transport. Standard HCI commands go to control EP0, events
//! return on the interface's interrupt IN endpoint.
use super::{Error, Interface, Result, Setup, UsbIo};
use crate::platform::{hex, number};
pub(super) fn matches(i: &Interface) -> bool {
    i.class == [0xe0, 1, 1]
}
fn hci(
    io: &mut impl UsbIo,
    slot: u8,
    i: Interface,
    opcode: u16,
    params: &[u8],
    out: &mut [u8; 260],
) -> Result<usize> {
    let mut packet = [0u8; 258];
    packet[..2].copy_from_slice(&opcode.to_le_bytes());
    packet[2] = params.len() as u8;
    packet[3..3 + params.len()].copy_from_slice(params);
    let len = (3 + params.len()) as u16;
    io.control(
        slot,
        Setup {
            kind: if i.number == 0 { 0x20 } else { 0x21 },
            request: 0,
            value: 0,
            index: u16::from(i.number),
            len,
        },
        &mut packet[..usize::from(len)],
    )?;
    for _ in 0..16 {
        let n = event(io, slot, i, out)?;
        if n >= 6 && out[0] == 0x0e && out[3..5] == opcode.to_le_bytes() {
            if out[5] != 0 {
                return Err(Error("HCI controller rejected command"));
            }
            return Ok(n);
        }
    }
    Err(Error("HCI completion missing"))
}
fn event(io: &mut impl UsbIo, slot: u8, i: Interface, out: &mut [u8; 260]) -> Result<usize> {
    let ep = i.endpoint(3, true)?;
    let n = io.transfer(slot, ep, &mut out[..257])?;
    if n < 2 || n != usize::from(out[1]) + 2 {
        return Err(Error("incomplete HCI event"));
    }
    Ok(n)
}
pub(super) fn command(io: &mut impl UsbIo, slot: u8, i: Interface, args: &[u8]) -> Result<()> {
    if !args.is_empty() && !args.eq_ignore_ascii_case(b"SCAN") {
        return Err(Error("BT or BT SCAN"));
    }
    let mut out = [0; 260];
    if hci(io, slot, i, 0x1001, &[], &mut out)? < 14 {
        return Err(Error("short local version"));
    }
    crate::puts("Bluetooth HCI version ");
    number(out[6].into());
    crate::puts("  manufacturer ");
    hex(u16::from_le_bytes([out[10], out[11]]).into(), 4);
    crate::puts("\n");
    if hci(io, slot, i, 0x1009, &[], &mut out)? < 12 {
        return Err(Error("short controller address"));
    }
    crate::puts("Controller ");
    address(&out[6..12]);
    crate::puts("\n");
    if args.is_empty() {
        return Ok(());
    }
    hci(io, slot, i, 0x200b, &[0, 0x20, 0, 0x10, 0, 0, 0], &mut out)?;
    hci(io, slot, i, 0x200c, &[1, 1], &mut out)?;
    let result = (|| {
        for _ in 0..8 {
            let n = event(io, slot, i, &mut out)?;
            if n >= 14 && out[0] == 0x3e && out[2] == 2 && out[3] != 0 {
                crate::puts("BLE advertiser ");
                address(&out[6..12]);
                crate::puts("\n");
                return Ok(());
            }
        }
        Err(Error("No advertising report received"))
    })();
    hci(io, slot, i, 0x200c, &[0, 0], &mut out)?;
    result
}
fn address(bytes: &[u8]) {
    for (n, b) in bytes.iter().rev().enumerate() {
        if n != 0 {
            crate::putc(b':');
        }
        hex((*b).into(), 2);
    }
}
