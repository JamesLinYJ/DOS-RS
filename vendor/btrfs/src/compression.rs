//! Decoding compressed extents, and encoding them for the write path.
//!
//! Btrfs may store an extent's data compressed, recording which
//! algorithm in the file-extent item. Three are defined, and a reader
//! that means to be useful on real volumes needs all three: zstd is the
//! usual choice on modern installs, zlib the long-standing default, and
//! LZO the one picked when decode speed matters most. The write path
//! encodes all three too (#265): [`compress`] produces what an extent
//! of each holds on disk, framing included, before it is padded to a
//! sector.
//!
//! # What is compressed, and what the offsets then mean
//!
//! The unit of compression is the whole extent, not the part of it a
//! given reference covers. So for a compressed extent:
//!
//! - `disk_bytenr` addresses the start of the compressed run, and
//!   `disk_num_bytes` is its length **on disk**;
//! - `ram_bytes` is what that run decodes to;
//! - `offset` and `num_bytes` then index into the **decoded** bytes.
//!
//! This is the trap. For an uncompressed extent the driver reads from
//! `disk_bytenr + offset`, because there the two coordinate systems
//! coincide. Doing that to a compressed extent seeks into the middle of
//! a compressed stream, and the whole run has to be decoded first and
//! sliced afterwards instead.
//!
//! # zlib and zstd carry their own framing; LZO does not
//!
//! A zlib or zstd extent is one ordinary stream of that format, so the
//! respective decoder handles it whole.
//!
//! LZO1X has no container — it is a bare instruction stream with no
//! length, no checksum and no end marker that a decoder can find without
//! being told how many bytes it holds. Btrfs therefore supplies its own
//! framing, and that framing is this module's real work. See
//! `decompress_lzo`.
//!
//! # A short decode is padded only on the file's last extent
//!
//! A decoder may stop early on a file's last extent, where the tail of
//! the final sector holds nothing, so [`decompress`] pads a short result
//! to `ram_bytes` there. Anywhere else it refuses: a stream that stops
//! early in an interior extent is truncated or damaged, and zeros are
//! legitimate file content, so padding one would hand a caller plausible
//! bytes with no way to tell. The caller says which case it has through
//! [`ShortDecode`] (#189).

use alloc::{vec::Vec};
use crate::error::{Error, Result};

/// Compression type as stored in `btrfs_file_extent_item.compression`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// Stored as written.
    None,
    /// zlib, i.e. DEFLATE inside a zlib wrapper.
    Zlib,
    /// LZO1X, in the segmented framing described in `decompress_lzo`.
    Lzo,
    /// zstd.
    Zstd,
}

impl Compression {
    /// Interpret the byte in a file-extent item.
    ///
    /// An unrecognised value is refused rather than treated as "none":
    /// reading a compressed extent as though it were plain produces
    /// bytes that look like a successful read of corrupt data, which no
    /// caller can distinguish from the real thing.
    pub fn from_byte(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Compression::None),
            1 => Ok(Compression::Zlib),
            2 => Ok(Compression::Lzo),
            3 => Ok(Compression::Zstd),
            other => Err(Error::UnsupportedFeature(format!(
                "extent uses compression type {other}, which is not one of \
                 zlib (1), LZO (2) or zstd (3)"
            ))),
        }
    }

    /// Whether data stored under this type needs decoding.
    pub fn is_compressed(self) -> bool {
        self != Compression::None
    }

    /// The byte a file-extent item records for this type.
    pub fn to_byte(self) -> u8 {
        match self {
            Compression::None => 0,
            Compression::Zlib => 1,
            Compression::Lzo => 2,
            Compression::Zstd => 3,
        }
    }

    /// What the compression property (`btrfs.compression`) asks for, read
    /// as the kernel reads it: a value beginning `zlib`, `lzo` or `zstd`
    /// names that algorithm (a level may follow, as in `zstd:3`), and `no`
    /// or `none` declines compression. `None` for anything else,
    /// including an empty value: no preference, so the inode's flags and
    /// the mount decide.
    pub fn from_property(value: &[u8]) -> Option<Self> {
        match value {
            b"no" | b"none" => Some(Compression::None),
            v if v.starts_with(b"zlib") => Some(Compression::Zlib),
            v if v.starts_with(b"lzo") => Some(Compression::Lzo),
            v if v.starts_with(b"zstd") => Some(Compression::Zstd),
            _ => None,
        }
    }
}

/// The most a compressed extent decodes to.
///
/// Btrfs compresses in units of 128 KiB (`BTRFS_MAX_UNCOMPRESSED`) and
/// never writes a larger one. Measured against btrfs-progs 6.x: every
/// compressed extent an 8 MiB file produced has `ram_bytes` of exactly
/// 131072, and none is larger.
pub const MAX_UNCOMPRESSED: usize = 128 * 1024;

/// The most a compressed extent occupies on disk.
///
/// `BTRFS_MAX_COMPRESSED`, the same number for the same reason.
/// Measured: a barely-compressible 128 KiB extent came out at 102400
/// bytes, and the encoder stores the extent uncompressed rather than
/// let it grow past the unit it started from.
pub const MAX_COMPRESSED: u64 = 128 * 1024;

/// Decode `input` into exactly `ram_bytes` bytes.
///
/// `sectorsize` is the filesystem's sector size, which the LZO framing
/// is laid out against and the other two ignore.
///
/// The decoded length is checked rather than trusted. Every caller then
/// slices this by an offset the item supplied, and a short result would
/// otherwise turn a corrupt extent into a panic or a silent hole.
/// What a decode that comes up short of `ram_bytes` means.
///
/// A decoder may legitimately stop early on the file's LAST extent, where
/// the tail of the final sector holds nothing. Anywhere else a short decode
/// is a truncated or damaged stream, and padding it with zeros hands back
/// plausible file content with no error — which is the one decode failure
/// this module used not to report (#189).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortDecode {
    /// The file's last extent: pad to `ram_bytes`.
    Pad,
    /// Any other extent: refuse.
    Refuse,
}

pub fn decompress(
    algo: Compression,
    input: &[u8],
    ram_bytes: usize,
    sectorsize: usize,
    short: ShortDecode,
) -> Result<Vec<u8>> {
    // `ram_bytes` IS THE ALLOCATION, and it comes off the disk.
    //
    // Every branch below sizes its output buffer from it before any
    // input is decoded, so an inline extent of about thirty bytes
    // claiming 2^55 asks for 36 petabytes. That does not fail politely:
    // `handle_alloc_error` aborts, and an abort is not something
    // `capi::guard`'s `catch_unwind` can turn into an EIO -- the host
    // process dies.
    //
    // Btrfs compresses in units of 128 KiB and never writes a larger
    // one. Measured rather than assumed: every compressed extent an
    // 8 MiB file produced under `compress-force=zstd` has `ram_bytes`
    // of exactly 131072, and none is larger.
    if ram_bytes > MAX_UNCOMPRESSED {
        return Err(Error::BadSuperblock(format!(
            "{algo:?} extent says it decodes to {ram_bytes} bytes, more than the \
             {MAX_UNCOMPRESSED} a compressed extent can hold"
        )));
    }
    let out = match algo {
        Compression::None => input.to_vec(),
        Compression::Zlib => decompress_zlib(input, ram_bytes)?,
        Compression::Lzo => decompress_lzo(input, ram_bytes, sectorsize)?,
        Compression::Zstd => decompress_zstd(input, ram_bytes)?,
    };

    // A decoder may legitimately stop early on the last extent of a
    // file, where the tail of the final sector holds nothing. Padding is
    // correct there; coming up short anywhere else is not, and the
    // difference is not visible from here — so pad and let the caller's
    // slice bound the result.
    if out.len() > ram_bytes {
        return Err(Error::BadSuperblock(format!(
            "{algo:?} extent decoded to {} bytes, more than the {ram_bytes} it records",
            out.len()
        )));
    }
    if out.len() < ram_bytes && short == ShortDecode::Refuse {
        return Err(Error::BadSuperblock(format!(
            "{algo:?} extent decoded to {} bytes where it records {ram_bytes}, and it is not \
             the file's last extent, so the rest is missing rather than absent",
            out.len()
        )));
    }
    let mut out = out;
    out.resize(ram_bytes, 0);
    Ok(out)
}

/// What an extent of `input` compressed with `algo` holds on disk, before
/// it is padded to a sector (#265), or `None` when `algo` is
/// [`Compression::None`] or its encoder produced something the kernel's
/// decoder would refuse. Whether the result is worth keeping -- it must
/// save a sector -- is the caller's decision, as it is the kernel's.
///
/// `sectorsize` is the filesystem's sector size, which the LZO framing is
/// laid out against and the other two ignore.
pub(crate) fn compress(algo: Compression, input: &[u8], sectorsize: usize) -> Option<Vec<u8>> {
    match algo {
        Compression::None => None,
        Compression::Zlib => Some(compress_zlib(input)),
        Compression::Lzo => compress_lzo(input, sectorsize),
        Compression::Zstd => compress_zstd(input),
    }
}

/// `input` as one zlib stream, at the level the kernel uses by default
/// (3): what a zlib-compressed extent holds on disk, before it is padded
/// to a sector (#265).
pub(crate) fn compress_zlib(input: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(input, 3)
}

/// The largest window a zstd extent's frame may declare: the kernel's
/// decoder is set up for frames of at most one compression unit, and
/// refuses a frame asking for more (`ZSTD_BTRFS_MAX_WINDOWLOG`, 17).
const ZSTD_MAX_WINDOW: u64 = MAX_UNCOMPRESSED as u64;

/// `input` as one zstd frame: what a zstd-compressed extent holds on
/// disk, before it is padded to a sector (#265).
///
/// ruzstd's encoder, at its fastest level (zstd's level 1; the kernel's
/// default is 3, which ruzstd does not implement -- the format is the
/// same, only the ratio differs). Its frame declares a 128 KiB window,
/// which is exactly what the kernel's decoder accepts; the frame header
/// is read back and checked, so that an encoder that ever declared more
/// would leave the extent stored uncompressed rather than written in a
/// form the kernel cannot read.
pub(crate) fn compress_zstd(input: &[u8]) -> Option<Vec<u8>> {
    let frame =
        ruzstd::encoding::compress_to_vec(input, ruzstd::encoding::CompressionLevel::Fastest);
    (zstd_window(&frame)? <= ZSTD_MAX_WINDOW).then_some(frame)
}

/// The window a zstd frame's header declares, or `None` for a header this
/// cannot read.
///
/// RFC 8878, section 3.1.1.1: the four-byte magic, the frame header
/// descriptor, then -- unless the descriptor's single-segment bit is set
/// -- a window descriptor of a 5-bit exponent and a 3-bit mantissa. A
/// single-segment frame's window is its content size, which follows the
/// dictionary id.
fn zstd_window(frame: &[u8]) -> Option<u64> {
    const MAGIC: [u8; 4] = 0xFD2F_B528u32.to_le_bytes();
    if frame.get(..4)? != MAGIC {
        return None;
    }
    let descriptor = *frame.get(4)?;
    if descriptor & 0x20 == 0 {
        let window = *frame.get(5)?;
        let log = 10 + u32::from(window >> 3);
        let base = 1u64.checked_shl(log)?;
        return Some(base + (base / 8) * u64::from(window & 7));
    }
    let dict_len = [0usize, 1, 2, 4][usize::from(descriptor & 3)];
    let at = 5 + dict_len;
    let size = match descriptor >> 6 {
        0 => u64::from(*frame.get(at)?),
        1 => u64::from(u16::from_le_bytes(frame.get(at..at + 2)?.try_into().ok()?)) + 256,
        2 => u64::from(u32::from_le_bytes(frame.get(at..at + 4)?.try_into().ok()?)),
        _ => u64::from_le_bytes(frame.get(at..at + 8)?.try_into().ok()?),
    };
    Some(size)
}

/// The most one LZO segment may hold: `lzo1x_worst_compress` of a sector,
/// the size of the buffer the kernel's decoder copies a segment into. A
/// longer one is refused as corrupt when it is read.
fn lzo_max_segment(sectorsize: usize) -> usize {
    sectorsize + sectorsize / 16 + 64 + 3
}

/// `input` in btrfs's segmented LZO framing, the inverse of
/// `decompress_lzo` (#265).
///
/// Each sector of input is compressed on its own as an LZO1X stream, by
/// the family's own permissive encoder, and stored behind a four-byte
/// little-endian length. After each segment, when what is left of the
/// sector cannot hold the next header -- one to three bytes -- those
/// bytes are zeroed and the next header starts on the boundary, so no
/// header straddles a sector. The extent opens with the total length,
/// those four bytes and any padding included.
///
/// `None` when a segment comes out longer than the kernel's decoder
/// takes one ([`lzo_max_segment`]), which only incompressible input
/// approaches and which would then be stored uncompressed anyway.
pub(crate) fn compress_lzo(input: &[u8], sectorsize: usize) -> Option<Vec<u8>> {
    if sectorsize < LZO_LEN {
        return None;
    }
    let mut out = vec![0u8; LZO_LEN];
    for sector in input.chunks(sectorsize) {
        let segment = lzo1x::compress(sector);
        if segment.len() > lzo_max_segment(sectorsize) {
            return None;
        }
        out.extend_from_slice(&u32::try_from(segment.len()).ok()?.to_le_bytes());
        out.extend_from_slice(&segment);
        let left = out.len().next_multiple_of(sectorsize) - out.len();
        if left < LZO_LEN {
            out.resize(out.len() + left, 0);
        }
    }
    let total = u32::try_from(out.len()).ok()?;
    out[..LZO_LEN].copy_from_slice(&total.to_le_bytes());
    Some(out)
}

fn decompress_zlib(input: &[u8], ram_bytes: usize) -> Result<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(input, ram_bytes).map_err(|e| {
        Error::BadSuperblock(format!(
            "zlib extent failed to decode ({:?}) after {} bytes",
            e.status,
            e.output.len()
        ))
    })
}

fn decompress_zstd(input: &[u8], ram_bytes: usize) -> Result<Vec<u8>> {
    use ruzstd::io::Read;
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(input)
        .map_err(|e| Error::BadSuperblock(format!("zstd frame: {e:?}")))?;
    let mut out = vec![0; ram_bytes];
    let mut used = 0;
    while used < out.len() {
        let n = decoder.read(&mut out[used..])
            .map_err(|e| Error::BadSuperblock(format!("zstd decode: {e:?}")))?;
        if n == 0 { break; }
        used += n;
    }
    out.truncate(used);
    Ok(out)
}

/// Length prefix width in the LZO framing.
const LZO_LEN: usize = 4;

/// Decode btrfs's segmented LZO framing.
///
/// LZO1X is a bare instruction stream: it carries no length, no checksum
/// and no terminator a decoder could find on its own. Btrfs supplies the
/// framing itself, and it is arranged so that a reader can decode any
/// one sector without walking the ones before it:
///
/// ```text
/// [ u32 total ][ u32 len ][ segment ][ u32 len ][ segment ] ... padding ...
/// |<--------------- sector ------------------>|<----- next sector ----->|
/// ```
///
/// - The extent opens with a little-endian `u32` giving the total
///   compressed length, counting those four bytes.
/// - Each segment is a little-endian `u32` of its own compressed length,
///   then that many bytes, decoding to at most one sector.
/// - **A segment's header never straddles a sector boundary.** When what
///   remains of the current sector cannot hold another four-byte header,
///   the rest of the sector (at most three bytes) is skipped and the next
///   header begins at the following boundary. The segment's DATA may run
///   across a boundary freely; the kernel's `lzo.c` says the same, and a
///   check that also required the data to fit would misread every extent
///   whose payload spans a sector.
///
/// That last rule is the whole reason this function exists rather than a
/// single call to the LZO decoder, and it is invisible in any extent
/// small enough to fit in one sector — which is most of them, and every
/// one that a hand-written test is likely to build. It is checked here
/// against extents the kernel itself compressed.
fn decompress_lzo(input: &[u8], ram_bytes: usize, sectorsize: usize) -> Result<Vec<u8>> {
    if sectorsize < LZO_LEN {
        return Err(Error::BadSuperblock(format!(
            "sector size {sectorsize} is too small to hold an LZO segment header"
        )));
    }
    if input.len() < LZO_LEN {
        return Err(Error::BadSuperblock(format!(
            "LZO extent is {} bytes, too short to hold its length header",
            input.len()
        )));
    }

    let total = read_u32(input, 0)? as usize;
    if total > input.len() {
        return Err(Error::BadSuperblock(format!(
            "LZO extent declares {total} compressed bytes but only {} are present",
            input.len()
        )));
    }

    let mut out = Vec::with_capacity(ram_bytes);
    let mut at = LZO_LEN;

    while at < total && out.len() < ram_bytes {
        // A header that will not fit in what is left of this sector means
        // the encoder moved on to the next one.
        let room = sectorsize - (at % sectorsize);
        if room < LZO_LEN {
            at += room;
            continue;
        }

        let seg_len = read_u32(input, at)? as usize;
        at += LZO_LEN;

        if seg_len == 0 {
            return Err(Error::BadSuperblock(
                "LZO extent contains a zero-length segment".into(),
            ));
        }
        let end = at.checked_add(seg_len).ok_or_else(|| {
            Error::BadSuperblock("LZO segment length overflows the extent".into())
        })?;
        if end > input.len() {
            return Err(Error::BadSuperblock(format!(
                "LZO segment at {at} runs {seg_len} bytes past the end of a {}-byte extent",
                input.len()
            )));
        }

        // Each segment decodes to at most one sector, and never to more
        // than what is left to produce.
        let want = sectorsize.min(ram_bytes - out.len());
        let chunk = lzo1x::decompress(&input[at..end], want).map_err(|e| {
            Error::BadSuperblock(format!("LZO segment at {at} failed to decode: {e}"))
        })?;
        out.extend_from_slice(&chunk);
        at = end;
    }

    Ok(out)
}

fn read_u32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().expect("4 bytes")))
        .ok_or_else(|| {
            Error::BadSuperblock(format!(
                "LZO extent ended at {} with a length prefix expected at {at}",
                b.len()
            ))
        })
}
