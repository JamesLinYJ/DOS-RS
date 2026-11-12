//! Synchronous DOS character-device requests. Transfer buffers borrow the caller's
//! memory for the call, including on kernels with 64-bit host addresses.

pub(super) const DONE: u16 = 0x0100;
pub(super) const UNKNOWN_COMMAND: u16 = 0x8103;

pub(super) struct Request<'a> {
    pub command: u8,
    pub status: u16,
    pub count: u16,
    pub media: u8,
    pub source: &'a [u8],
    pub destination: &'a mut [u8],
}

pub(super) fn write(bytes: &[u8], driver: fn(&mut Request<'_>)) {
    for chunk in bytes.chunks(u16::MAX.into()) {
        driver(&mut Request {
            command: 8,
            status: 0,
            count: chunk.len() as u16,
            media: 0,
            source: chunk,
            destination: &mut [],
        });
    }
}
