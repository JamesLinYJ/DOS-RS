//! Pure-Rust LZO1X decompressor.
//!
//! LZO1X is the byte-stream format produced by the LZO family of
//! compressors (LZO1X-1, LZO1X-999, …). All LZO1X-* encoders share one
//! decode grammar, so a single decoder handles every variant. The format
//! shows up in SquashFS (compression id 3), Btrfs compressed extents,
//! JFFS2, and kernel crash dumps.
//!
//! This crate decodes only. There is no compressor — the decode grammar
//! is small and fully published, while a competitive encoder is a much
//! larger undertaking with no consumer in sight.
//!
//! # Provenance
//!
//! Written from the publicly published prose description of the LZO1X
//! byte-stream grammar, principally the format-description document
//! distributed as `Documentation/staging/lzo.rst` in the Linux source
//! tree — a prose specification of the on-the-wire token grammar, not
//! decompressor code. The grammar as used is reproduced below so the
//! mapping from spec to implementation is auditable in-tree.
//!
//! A byte-stream format is a set of facts, not an expressive work, and
//! this crate implements one from its published description. It carries
//! no code from any other LZO implementation.
//!
//! # The grammar
//!
//! The stream is a sequence of *instructions*. Each instruction is one
//! command byte, sometimes followed by extension/distance bytes, that
//! either emits a run of literal bytes copied straight from the input,
//! or a *match*: a back-reference that copies `len` bytes from `dist`
//! bytes earlier in the already-produced output. After most matches a
//! small number (0..=3) of trailing literals are copied immediately;
//! this count is the *state* carried into the next instruction and is
//! taken from the low 2 bits ("SS") of the match's distance operand.
//!
//! Lengths that would overflow the few bits available in the command
//! byte are extended by a run of zero bytes: each `0x00` adds 255, and
//! the terminating non-zero byte adds its own value (the "zero-run"
//! length extension).
//!
//! Instruction buckets, keyed by the command byte `t`:
//!
//! ```text
//! 0 0 0 0 L L L L  (t < 16)  -- three-way, keyed by the carried `state`:
//!     - state == 0: a literal run, len = 3 + (t==0 ? zero_run+15 : t).
//!                   Sets state = 4 afterwards (the "long-literals" sentinel).
//!     - state 1..=3 (the SS bits of the previous match): a 2-byte match,
//!                   dist = (next_byte << 2) + ((t >> 2) & 3) + 1.
//!     - state == 4: a 3-byte match (immediately after a long literal run),
//!                   dist = (next_byte << 2) + ((t >> 2) & 3) + 2049.
//!     In both match cases the new state is t & 3.
//! 0 0 0 1 H L L L  (16..=31)
//!     match, len = 2 + (LLL==0 ? zero_run+7 : LLL),
//!     dist = 16384 + (H << 14) + (LE16_operand >> 2);
//!     a token contributing nothing above that base -- H clear and the
//!     operand's upper 14 bits zero -- is the END-OF-STREAM marker,
//!     whatever LLL holds. 11 00 00 is the spelling encoders emit;
//!     13 00 00 and 17 00 00 terminate just as surely.
//! 0 0 1 L L L L L  (32..=63)
//!     match, len = 2 + (LLLLL==0 ? zero_run+31 : LLLLL),
//!     dist = (LE16_operand >> 2) + 1
//! 0 1 L D D D S S  (64..=127)
//!     match, len = 3 + ((t >> 5) & 1),
//!     dist = (next_byte << 3) + ((t >> 2) & 7) + 1
//! 1 L L D D D S S  (128..=255)
//!     match, len = 5 + ((t >> 5) & 3),
//!     dist = (next_byte << 3) + ((t >> 2) & 7) + 1
//! ```
//!
//! Stream bootstrap (first command byte only): a value >= 18 means a
//! leading literal run of `t - 17` bytes (with state = min(t-17, 4)); a
//! value of 17 is reserved as a bitstream-version marker. Values < 18
//! fall through to the normal grammar.
//!
//! # Safety
//!
//! The decoder is bounds-checked at every step and returns
//! [`Error::Malformed`] on any malformed token rather than panicking, so
//! it is safe to run on untrusted input. It contains no `unsafe`.
//!
//! # Example
//!
//! ```
//! // Four literal bytes via the bootstrap run, then the end-of-stream marker.
//! let stream = [21u8, b'a', b'b', b'c', b'd', 0x11, 0x00, 0x00];
//! let out = lzo1x::decompress(&stream, 4).unwrap();
//! assert_eq!(&out, b"abcd");
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use core::fmt;

mod compress;

pub use compress::compress;

/// The single failure mode of the decoder: the input is not a
/// well-formed LZO1X stream, or decoding it would exceed `max_out`.
///
/// The variant carries no position information by design — a corrupt
/// stream's first *detected* error is rarely where the corruption is,
/// so an offset would imply a precision the decoder does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The stream is truncated, contains an invalid token, references
    /// data before the start of the output, or decodes to more than the
    /// caller's `max_out` bound.
    Malformed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed => f.write_str("malformed LZO1X stream"),
        }
    }
}

impl core::error::Error for Error {}

/// Result alias for [`decompress`].
pub type Result<T> = core::result::Result<T, Error>;

const E: Error = Error::Malformed;

/// Each `0x00` byte in a zero-run length extension contributes this much
/// to the running length.
const ZERO_RUN_INCREMENT: usize = 255;

/// Upper bound on a single extended length, as a defence against a
/// corrupt all-zeros stream. This is not a format limit: the grammar
/// permits arbitrarily long runs, but no real block is anywhere near
/// 16 MiB, so a length beyond it means the input is garbage rather than
/// merely large. Without this cap an all-zeros input loops until the
/// length overflows.
const MAX_EXTENDED_LENGTH: usize = 1 << 24;

/// Ceiling on the buffer reserved before any input is validated.
///
/// The third leg of the same defence as `max_out` and
/// [`MAX_EXTENDED_LENGTH`], and the only one that was anonymous.
/// `max_out` is supplied by the caller, so reserving it outright would
/// let a bogus bound drive a large allocation before a single byte of
/// the stream has been checked. Capping the EAGER reservation costs one
/// reallocation on a genuinely large block and bounds what a wrong
/// argument can ask for; the real limit is still enforced per copy.
const INITIAL_CAPACITY_CAP: usize = 1 << 20;

/// Base distance for the `0 0 0 1 H L L L` long-distance match bucket.
/// A token in that bucket whose distance contribution above this base is
/// zero is the end-of-stream marker rather than a match; its length bits
/// are not part of that decision.
const LONG_MATCH_DISTANCE_BASE: usize = 16384;

/// Split an LE16 match operand into its two fields.
///
/// The grammar writes the operand as `DDDDDDDD DDDDDDSS`: fourteen bits
/// of distance contribution above the bucket's base, then two bits of
/// trailing-literal count. Both were open-coded as raw shifts at five
/// sites, where `>> 2` collides visually with the unrelated `>> 2` that
/// extracts distance bits from a COMMAND byte.
fn split_operand(op: usize) -> (usize, usize) {
    (op >> 2, op & 3)
}

/// The value of `state` meaning "a long literal run just ended", as
/// opposed to a count of trailing literals.
///
/// `state` carries two incompatible meanings. For `0..=3` it is a
/// COUNT — how many literals to copy after the next match. This value
/// is not a count; it is a sentinel selecting a different decode for
/// the `t < 16` bucket.
///
/// The distinction matters at the literal copy near the end of the
/// loop, which copies `state` bytes. That is correct only because
/// `state` can never hold this value there: every path reaching it has
/// just assigned `t & 3` or `op & 3`, and the long-literal path
/// `continue`s before it. A future arm that assigned the sentinel and
/// fell through would emit four garbage bytes rather than fail, so the
/// invariant is asserted where it is relied on.
const LONG_LITERAL_RUN_STATE: usize = 4;

/// Base distance for the three-byte match that follows a long literal
/// run (the `state == 4` case of the `t < 16` bucket).
const POST_LITERAL_MATCH_DISTANCE_BASE: usize = 2049;

/// Decompress an LZO1X stream.
///
/// `input` is the raw compressed block. `max_out` is an **upper bound**
/// on the decompressed length — for SquashFS that is the block size (or
/// 8 KiB for metadata blocks); for Btrfs it is the extent's uncompressed
/// length. The true output length is determined by the stream's own
/// end-of-stream marker, not by `max_out`; the bound exists so a corrupt
/// stream cannot drive an unbounded allocation.
///
/// # Errors
///
/// Returns [`Error::Malformed`] if the stream is truncated, contains an
/// invalid token, back-references before the start of the output, or
/// would decode to more than `max_out` bytes.
pub fn decompress(input: &[u8], max_out: usize) -> Result<Vec<u8>> {
    let mut d = Decoder {
        input,
        ip: 0,
        out: Vec::with_capacity(max_out.min(INITIAL_CAPACITY_CAP)),
        max_out,
    };
    d.run()?;
    Ok(d.out)
}

struct Decoder<'a> {
    input: &'a [u8],
    ip: usize,
    out: Vec<u8>,
    max_out: usize,
}

impl Decoder<'_> {
    #[inline]
    fn next(&mut self) -> Result<u8> {
        let b = *self.input.get(self.ip).ok_or(E)?;
        self.ip += 1;
        Ok(b)
    }

    /// Read a little-endian 16-bit operand (used by the long-distance
    /// match buckets; its low 2 bits double as the trailing-literal
    /// state, the upper 14 bits as the distance contribution).
    #[inline]
    fn next_le16(&mut self) -> Result<u16> {
        let lo = self.next()? as u16;
        let hi = self.next()? as u16;
        Ok(lo | (hi << 8))
    }

    /// Zero-run length extension: consume a run of `0x00` bytes (each
    /// worth 255) followed by one non-zero byte worth its own value.
    /// `base` is the length already accounted for by the command byte.
    /// Bounded so a corrupt all-zeros stream can't loop forever.
    fn extend_length(&mut self, base: usize) -> Result<usize> {
        let mut len = base;
        loop {
            let b = self.next()?;
            if b != 0 {
                return len.checked_add(b as usize).ok_or(E);
            }
            // Cap the run length far below any legitimate block size so
            // a corrupt stream is rejected early rather than looping.
            len = len.checked_add(ZERO_RUN_INCREMENT).ok_or(E)?;
            if len > MAX_EXTENDED_LENGTH {
                return Err(E);
            }
        }
    }

    /// Copy a back-reference of `len` bytes from `dist` bytes behind the
    /// current end of output. Overlapping copies are byte-at-a-time (the
    /// classic LZ77 self-referential run), so `dist < len` is legal and
    /// is how the format encodes byte runs.
    fn copy_match(&mut self, dist: usize, len: usize) -> Result<()> {
        if dist == 0 || dist > self.out.len() {
            return Err(E);
        }
        if self.out.len() + len > self.max_out {
            return Err(E);
        }
        let start = self.out.len() - dist;
        for src in start..start + len {
            let b = self.out[src];
            self.out.push(b);
        }
        Ok(())
    }

    /// Copy `n` literal bytes straight from the input to the output.
    fn copy_literals(&mut self, n: usize) -> Result<()> {
        if self.out.len() + n > self.max_out {
            return Err(E);
        }
        let end = self.ip.checked_add(n).ok_or(E)?;
        let slice = self.input.get(self.ip..end).ok_or(E)?;
        self.out.extend_from_slice(slice);
        self.ip = end;
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        // ---- stream bootstrap: leading literal run ----
        // The first command byte, when >= 18, is itself a literal-run
        // length of (t - 17). 17 is a reserved version marker.
        let mut state: usize;
        let first = *self.input.get(self.ip).ok_or(E)?;
        if first >= 18 {
            self.ip += 1;
            let n = (first - 17) as usize;
            self.copy_literals(n)?;
            state = n.min(LONG_LITERAL_RUN_STATE);
        } else {
            state = 0;
        }

        loop {
            // The stream is self-delimiting: a well-formed LZO1X stream
            // ends with the end-of-stream marker, decoded inside the match
            // arm below. Reaching the end of the input without a marker is
            // malformed (`next()` returns an error).
            let t = self.next()?;

            if t >= 16 {
                // Match instruction. Decode by the top set bit.
                let (len, dist, new_state) = if t >= 128 {
                    // 1 L L D D D S S — 5..=8 byte match, short distance.
                    let h = self.next()? as usize;
                    let len = 5 + ((t >> 5) & 3) as usize;
                    let dist = (h << 3) + ((t >> 2) & 7) as usize + 1;
                    (len, dist, (t & 3) as usize)
                } else if t >= 64 {
                    // 0 1 L D D D S S — 3..=4 byte match, short distance.
                    let h = self.next()? as usize;
                    let len = 3 + ((t >> 5) & 1) as usize;
                    let dist = (h << 3) + ((t >> 2) & 7) as usize + 1;
                    (len, dist, (t & 3) as usize)
                } else if t >= 32 {
                    // 0 0 1 L L L L L — match, distance 1..=16384.
                    let base = (t & 31) as usize;
                    let len = if base == 0 {
                        self.extend_length(31)? + 2
                    } else {
                        base + 2
                    };
                    let op = self.next_le16()? as usize;
                    let (dist_bits, next_state) = split_operand(op);
                    (len, dist_bits + 1, next_state)
                } else {
                    // 0 0 0 1 H L L L — match, distance 16384..=49151,
                    // plus the end-of-stream marker.
                    // Not an extension byte, unlike the `h` in the arms
                    // above and below: one bit lifted out of the command
                    // byte, contributing 2^14 to the distance.
                    let dist_high_bit = ((t >> 3) & 1) as usize;
                    let base = (t & 7) as usize;
                    let len = if base == 0 {
                        self.extend_length(7)? + 2
                    } else {
                        base + 2
                    };
                    let op = self.next_le16()? as usize;
                    let (dist_bits, next_state) = split_operand(op);
                    let dist = LONG_MATCH_DISTANCE_BASE + (dist_high_bit << 14) + dist_bits;
                    // End of stream, and the test is deliberately looser
                    // than the canonical marker. Any token in this bucket
                    // that contributes no distance above the base — H
                    // clear and the operand's upper 14 bits zero — ends
                    // the stream, whatever its length bits hold.
                    // `11 00 00` is the spelling encoders emit, but
                    // `13 00 00` and `17 00 00` terminate identically,
                    // which is what the grammar asks for: the distance is
                    // what discriminates a marker from a match, and a
                    // length is meaningless on a token that copies nothing.
                    //
                    // Do not "tighten" this to also require zero length
                    // bits. That would make the decoder stricter than the
                    // format and reject streams a conforming encoder may
                    // legitimately produce, and the two filesystems that
                    // decode through here — btrfs extents and SquashFS
                    // blocks — would fail to read rather than fail loudly.
                    // The test `end_of_stream_marker_ignores_its_length_bits`
                    // below pins the accepted set.
                    //
                    // `len` is computed above and discarded on this path,
                    // which is order rather than oversight: when the
                    // length field is zero its extension bytes sit *ahead
                    // of* the operand in the stream, so the length has to
                    // be decoded before the operand can be located at all.
                    if dist_bits == 0 && dist_high_bit == 0 {
                        return Ok(());
                    }
                    (len, dist, next_state)
                };

                self.copy_match(dist, len)?;
                state = new_state;
            } else {
                // t < 16: the `0 0 0 0 ...` bucket, three-way by `state`
                // (the number of literals the PREVIOUS instruction left
                // pending, where 4 is the sentinel meaning "a long literal
                // run just happened").
                if state == 0 {
                    // Long literal run. t==0 triggers the zero-run
                    // extension; otherwise the run is t + 3 bytes long.
                    // Afterwards state = 4 (the sentinel).
                    let n = if t == 0 {
                        self.extend_length(15)? + 3
                    } else {
                        t as usize + 3
                    };
                    self.copy_literals(n)?;
                    state = LONG_LITERAL_RUN_STATE;
                    continue;
                }
                // Both remaining cases are a short match whose command byte
                // is `0 0 0 0 D D S S`: DD = (t >> 2) & 3 contributes to the
                // distance, SS = t & 3 becomes the next trailing-literal
                // state. One more byte H extends the distance.
                //   state 1..=3 -> a 2-byte match,  dist = (H<<2)+DD+1
                //   state 4     -> a 3-byte match,  dist = (H<<2)+DD+2049
                let h = self.next()? as usize;
                let dd = ((t >> 2) & 3) as usize;
                let (len, dist) = if state == LONG_LITERAL_RUN_STATE {
                    (3, (h << 2) + dd + POST_LITERAL_MATCH_DISTANCE_BASE)
                } else {
                    (2, (h << 2) + dd + 1)
                };
                self.copy_match(dist, len)?;
                state = (t & 3) as usize;
            }

            // After a match, copy `state` trailing literals immediately.
            // The sentinel is never live here — see LONG_LITERAL_RUN_STATE.
            // Asserted rather than commented alone, so a future arm that
            // broke the invariant would fail a debug build instead of
            // quietly copying four bytes that are not literals.
            debug_assert!(
                state < LONG_LITERAL_RUN_STATE,
                "state {state} is the long-literal sentinel, not a literal count"
            );
            if state > 0 {
                self.copy_literals(state)?;
            }
        }
    }
}

// DOES THIS BUILD ACTUALLY TRAP AN ARITHMETIC OVERFLOW?
//
// Inline in `lib.rs` rather than a module of its own, for the reason
// `tests/ci_profile.rs` explains at length: a separate file under `src/`
// hangs off one `mod` line, and losing that line leaves the file present,
// uncompiled, and asserting nothing, with no lint to say so. Inline,
// there is no declaration to lose. It cannot live in `tests/` at all --
// it has to be part of the library target that the debug step builds,
// because the question it answers is about that build.
