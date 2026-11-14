//! USB mass storage Bulk-Only Transport with SCSI transparent commands.
use super::{Error, Interface, Result, UsbIo};
pub(super) fn matches(i: &Interface) -> bool {
    i.class == [8, 6, 0x50]
}
fn scsi(
    io: &mut impl UsbIo,
    slot: u8,
    i: Interface,
    tag: u32,
    cdb: &[u8],
    buf: &mut [u8],
) -> Result<()> {
    let tx = i.endpoint(2, false)?;
    let rx = i.endpoint(2, true)?;
    let mut cbw = [0u8; 31];
    cbw[..4].copy_from_slice(b"USBC");
    cbw[4..8].copy_from_slice(&tag.to_le_bytes());
    cbw[8..12].copy_from_slice(&(buf.len() as u32).to_le_bytes());
    cbw[12] = 0x80;
    cbw[14] = cdb.len() as u8;
    cbw[15..15 + cdb.len()].copy_from_slice(cdb);
    if io.transfer(slot, tx, &mut cbw)? != 31 {
        return Err(Error("short command wrapper"));
    }
    let length = if buf.is_empty() {
        0
    } else {
        io.transfer(slot, rx, buf)?
    };
    let mut csw = [0; 13];
    if io.transfer(slot, rx, &mut csw)? != 13
        || &csw[..4] != b"USBS"
        || csw[4..8] != tag.to_le_bytes()
    {
        return Err(Error("invalid status wrapper"));
    }
    if csw[12] != 0 || csw[8..12] != [0; 4] || length != buf.len() {
        return Err(Error("SCSI command failed"));
    }
    Ok(())
}
pub(super) fn read(
    io: &mut impl UsbIo,
    slot: u8,
    i: Interface,
    lba: u32,
    out: &mut [u8; 512],
) -> Result<()> {
    // Inquiry and request sense clear possible media-change unit attention.
    scsi(io, slot, i, 1, &[0x12, 0, 0, 0, 36, 0], &mut [0; 36])?;
    if scsi(io, slot, i, 2, &[0; 6], &mut []).is_err() {
        scsi(io, slot, i, 3, &[3, 0, 0, 0, 18, 0], &mut [0; 18])?;
    }
    let mut capacity = [0; 8];
    scsi(
        io,
        slot,
        i,
        4,
        &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        &mut capacity,
    )?;
    if capacity[4..] != 512u32.to_be_bytes()
        || lba > u32::from_be_bytes(capacity[..4].try_into().unwrap())
    {
        return Err(Error("unsupported sector size or LBA"));
    }
    let mut cdb = [0u8; 10];
    cdb[0] = 0x28;
    cdb[2..6].copy_from_slice(&lba.to_be_bytes());
    cdb[8] = 1;
    scsi(io, slot, i, 5, &cdb, out)
}
