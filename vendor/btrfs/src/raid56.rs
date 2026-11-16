//! RAID5 and RAID6: where a data element lives, and how a lost one is
//! rebuilt from the rest of its full stripe (#268).
//!
//! # The layout
//!
//! A parity chunk has `n` stripes, one per device, of which `k` hold data
//! in any one full stripe: `n - 1` for RAID5 and `n - 2` for RAID6. The
//! chunk's logical range is cut into `stripe_len` elements; `k` of them in
//! a row make a full stripe, and full stripe `f` occupies the same
//! `stripe_len` window, `f * stripe_len` into every one of the `n` stripes.
//! Within it, data element `i` is on stripe `(i + f) mod n`, P on
//! `(k + f) mod n` and, for RAID6, Q on `(k + 1 + f) mod n`: parity
//! rotates one stripe per full stripe, so no one device holds all of it.
//!
//! # The parity
//!
//! P is the XOR of the data elements. Q is `sum(g^i * D_i)` over GF(2^8)
//! with the polynomial `x^8 + x^4 + x^3 + x^2 + 1` (0x11d) and generator
//! `g = 2`, the code the Linux RAID6 driver uses and H. Peter Anvin's
//! paper *The mathematics of RAID-6* derives. Both are checked against
//! pools the kernel wrote in `tests/raid56_oracle.rs`, which is the only
//! evidence that counts: the unit tests below generate parity with the
//! same arithmetic that rebuilds from it.
//!
//! # Rebuilding is a read attempt, not a copy
//!
//! A mirrored chunk has copies; a parity chunk has one copy and several
//! ways of computing it again. Each is numbered as a read attempt after
//! the direct read, so the read path's fallback — try the next attempt
//! when a checksum fails — works unchanged:
//!
//! | attempt | reads |
//! |---|---|
//! | 0 | the element itself |
//! | 1 | the other data elements and P |
//! | 2 (RAID6) | the other data elements and Q |
//! | 3.. (RAID6) | P and Q and every data element but this one and one other, both rebuilt together |
//!
//! Attempts 3 onwards pair the wanted element with each other data
//! element in turn, because which second element is also bad is not
//! something a reader can know; a checksum on the result says whether
//! the guess was right.

use alloc::{vec::Vec};
use crate::error::{Error, Result};

/// `x^8 + x^4 + x^3 + x^2 + 1`, the low byte: the RAID6 field polynomial.
const POLY: u16 = 0x11d;

/// `g^i` for `i` in `0..255`, and `log[g^i] = i`.
struct Tables {
    exp: [u8; 255],
    log: [u8; 256],
}

const fn tables() -> Tables {
    let mut exp = [0u8; 255];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= POLY;
        }
        i += 1;
    }
    Tables { exp, log }
}

static GF: Tables = tables();

/// `g^e`, for any exponent.
fn pow_g(e: usize) -> u8 {
    GF.exp[e % 255]
}

/// `a * b` in GF(2^8).
fn mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    GF.exp[(usize::from(GF.log[a as usize]) + usize::from(GF.log[b as usize])) % 255]
}

/// `1 / a` in GF(2^8); `a` is never zero here.
fn inv(a: u8) -> u8 {
    GF.exp[(255 - usize::from(GF.log[a as usize])) % 255]
}

/// Where one element of a full stripe is: the stripe index in the chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Which stripe each data element is on, by data index.
    pub data_stripes: Vec<usize>,
    /// The stripe P is on.
    pub p: usize,
    /// The stripe Q is on, for RAID6.
    pub q: Option<usize>,
}

/// The most stripes a parity chunk is read with. The kernel caps a chunk
/// at far fewer devices than this in practice; a chunk claiming more is
/// refused rather than allocated for.
pub const MAX_STRIPES: usize = 256;

/// Where everything in full stripe `full` of an `n`-stripe chunk with
/// `parity` parity elements lives.
pub fn placement(n: usize, parity: usize, full: u64) -> Result<Placement> {
    if n > MAX_STRIPES || n <= parity {
        return Err(Error::BadChunkItem(format!(
            "a parity chunk with {n} stripes and {parity} parity elements"
        )));
    }
    let k = n - parity;
    let rot = (full % n as u64) as usize;
    Ok(Placement {
        data_stripes: (0..k).map(|i| (i + rot) % n).collect(),
        p: (k + rot) % n,
        q: (parity == 2).then_some((k + 1 + rot) % n),
    })
}

/// How a lost data element is computed again; see the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rebuild {
    /// From the other data elements and P.
    P,
    /// From the other data elements and Q.
    Q,
    /// From P and Q, with data element `other` lost as well.
    PQ {
        /// The second lost data element.
        other: usize,
    },
}

impl Rebuild {
    /// The rebuild read attempt `attempt` (1 onwards) uses for data
    /// element `want` of `data`, or `None` past the last.
    pub fn for_attempt(attempt: usize, want: usize, data: usize, raid6: bool) -> Option<Self> {
        match attempt {
            1 => Some(Rebuild::P),
            2 if raid6 => Some(Rebuild::Q),
            a if raid6 && a >= 3 => {
                let nth = a - 3;
                (0..data)
                    .filter(|&i| i != want)
                    .nth(nth)
                    .map(|other| Rebuild::PQ { other })
            }
            _ => None,
        }
    }

    /// How many read attempts a parity chunk with `data` data elements
    /// offers, the direct read included.
    pub fn attempts(data: usize, raid6: bool) -> usize {
        if raid6 {
            3 + data.saturating_sub(1)
        } else {
            2
        }
    }
}

/// Data element `want`, computed from the full stripe's other elements.
///
/// `data[i]` is data element `i` as read, `None` for the ones the method
/// does not read (`want`, and `other` for [`Rebuild::PQ`]); `p` and `q`
/// likewise. Every present element is the same length.
pub fn rebuild(
    method: Rebuild,
    want: usize,
    data: &[Option<Vec<u8>>],
    p: Option<&[u8]>,
    q: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let missing = |what: &str| Error::BadChunkItem(format!("a parity rebuild without {what}"));
    match method {
        Rebuild::P => {
            let mut out = p.ok_or_else(|| missing("P"))?.to_vec();
            for (i, d) in data.iter().enumerate() {
                if i == want {
                    continue;
                }
                xor_into(
                    &mut out,
                    d.as_deref().ok_or_else(|| missing("a data element"))?,
                );
            }
            Ok(out)
        }
        Rebuild::Q => {
            // Q ^ sum over the others of g^i * D_i is g^want * D_want.
            let mut acc = q.ok_or_else(|| missing("Q"))?.to_vec();
            for (i, d) in data.iter().enumerate() {
                if i == want {
                    continue;
                }
                mul_xor_into(
                    &mut acc,
                    d.as_deref().ok_or_else(|| missing("a data element"))?,
                    pow_g(i),
                );
            }
            let scale = inv(pow_g(want));
            for b in &mut acc {
                *b = mul(*b, scale);
            }
            Ok(acc)
        }
        Rebuild::PQ { other } => {
            if other == want {
                return Err(missing("a second element to rebuild"));
            }
            // With x = want and y = other lost:
            //   Pxy = P ^ sum D_i           = D_x ^ D_y
            //   Qxy = Q ^ sum g^i D_i       = g^x D_x ^ g^y D_y
            // so (g^x ^ g^y) D_x = g^y Pxy ^ Qxy.
            let mut pxy = p.ok_or_else(|| missing("P"))?.to_vec();
            let mut qxy = q.ok_or_else(|| missing("Q"))?.to_vec();
            for (i, d) in data.iter().enumerate() {
                if i == want || i == other {
                    continue;
                }
                let d = d.as_deref().ok_or_else(|| missing("a data element"))?;
                xor_into(&mut pxy, d);
                mul_xor_into(&mut qxy, d, pow_g(i));
            }
            let (gx, gy) = (pow_g(want), pow_g(other));
            let scale = inv(gx ^ gy);
            Ok(pxy
                .iter()
                .zip(&qxy)
                .map(|(&pb, &qb)| mul(mul(gy, pb) ^ qb, scale))
                .collect())
        }
    }
}

/// P, and for RAID6 Q, of a full stripe's data elements in data order:
/// what its parity elements hold when nothing in it is damaged. Every
/// element is the same length.
///
/// Scrub recomputes these to find a damaged parity element (#301), which
/// no read ever looks at while the data verifies.
pub fn parity(data: &[&[u8]], raid6: bool) -> (Vec<u8>, Option<Vec<u8>>) {
    let len = data.first().map_or(0, |d| d.len());
    let mut p = vec![0u8; len];
    for d in data {
        xor_into(&mut p, d);
    }
    let q = raid6.then(|| {
        let mut q = vec![0u8; len];
        for (i, d) in data.iter().enumerate() {
            mul_xor_into(&mut q, d, pow_g(i));
        }
        q
    });
    (p, q)
}

fn xor_into(acc: &mut [u8], d: &[u8]) {
    for (a, b) in acc.iter_mut().zip(d) {
        *a ^= b;
    }
}

fn mul_xor_into(acc: &mut [u8], d: &[u8], factor: u8) {
    for (a, b) in acc.iter_mut().zip(d) {
        *a ^= mul(*b, factor);
    }
}
