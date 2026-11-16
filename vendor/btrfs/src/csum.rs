//! The checksum tree: what the filesystem says its data should hash to.
//!
//! Btrfs checksums data as well as metadata. Every sector of every
//! ordinary data extent has a digest filed in a tree of its own, keyed
//! by the sector's logical address, and the kernel verifies each one on
//! the way out. A driver that skips the check hands back bit-rotted or
//! clobbered bytes with a successful return — the one failure a caller
//! cannot detect, and the one this crate's module docs say it exists to
//! prevent.
//!
//! ## How the tree is keyed
//!
//! One objectid, one type, and the logical address in the offset:
//!
//! ```text
//! objectid = BTRFS_EXTENT_CSUM_OBJECTID  (-10, i.e. u64::MAX - 9)
//! type     = BTRFS_EXTENT_CSUM_KEY       (128)
//! offset   = logical address of the first sector the item covers
//! ```
//!
//! An item's data is a packed array of digests, `csum_size` bytes each,
//! covering consecutive sectors from `offset` onwards. So one item can
//! describe a long run, and the item covering a given sector is the
//! **last** one whose offset is at or below it — not necessarily one
//! filed at exactly that address. Looking up an exact key finds a
//! checksum only when the sector happens to start a run, which is the
//! easy mistake to make here and would leave most sectors silently
//! unchecked.
//!
//! ## What absence means
//!
//! Not every sector has a digest, and a missing one is not a fault:
//!
//! - a file marked `NODATASUM` has none by definition;
//! - `NODATACOW` implies `NODATASUM`, because a block rewritten in
//!   place cannot keep a checksum consistent with itself;
//! - a preallocated extent holds no data yet.
//!
//! So absence is "nothing to check", and only a digest that is present
//! and does not match is an error. That asymmetry is the whole policy:
//! the tree is authoritative about the sectors it names and silent
//! about the rest.

use alloc::{vec::Vec};
use alloc::collections::BTreeMap;

use crate::btree::Tree;
use crate::chunk::DiskKey;
use crate::error::Result;

/// `BTRFS_CSUM_TREE_OBJECTID` — the root tree's entry for this tree.
pub const CSUM_TREE_OBJECTID: u64 = 7;

/// `BTRFS_EXTENT_CSUM_KEY`.
pub const EXTENT_CSUM_KEY: u8 = 128;

/// `BTRFS_EXTENT_CSUM_OBJECTID`, which the kernel spells `-10`.
///
/// Keys are compared as unsigned, so this sorts near the top of the
/// tree; it is a constant tag rather than a number with arithmetic
/// meaning.
pub const EXTENT_CSUM_OBJECTID: u64 = u64::MAX - 9;

/// The digests covering a run of sectors, by the logical address of the
/// sector each one describes.
///
/// Only sectors the tree actually names are present. See the module doc
/// for why a missing sector is not a failure.
pub type SectorDigests = BTreeMap<u64, Vec<u8>>;

/// Every digest the csum tree holds for the sectors in
/// `[start, start + len)`.
///
/// `start` and `len` are expected to be sector-aligned; a caller reading
/// a sub-range widens it to sector boundaries first, because a digest
/// covers a whole sector and there is nothing to compare a fragment
/// against.
///
/// The scan begins at the item covering `start` rather than at `start`
/// itself. A `for_each_from` seek lands on the first key at or after
/// its target, so seeking to `start` steps *over* the item that covers
/// it whenever that item begins earlier — which is the common case, as
/// one item covers a long run. The leaf holding the seek point is
/// examined directly to find the item at or before `start`, and the
/// walk then continues from that item's key.
pub(crate) fn digests_for_range(
    tree: &Tree,
    root: u64,
    csum_size: usize,
    sectorsize: u64,
    start: u64,
    len: u64,
) -> Result<SectorDigests> {
    let mut out = SectorDigests::new();
    if len == 0 || csum_size == 0 || sectorsize == 0 {
        return Ok(out);
    }
    let end = start.saturating_add(len);

    let target = DiskKey {
        objectid: EXTENT_CSUM_OBJECTID,
        key_type: EXTENT_CSUM_KEY,
        offset: start,
    };

    // Where to begin the walk: the item covering `start`, if the leaf
    // that would hold `start` has one before it.
    let mut from = target;
    let leaf = tree.descend(root, &target)?;
    if let Some(items) = leaf.body.items() {
        for item in items {
            if item.key.objectid != EXTENT_CSUM_OBJECTID
                || item.key.key_type != EXTENT_CSUM_KEY
                || item.key.offset > start
            {
                continue;
            }
            if item.key.offset >= from.offset || from.offset == start {
                from = item.key;
            }
        }
    }

    tree.for_each_from(root, &from, &mut |key: &DiskKey, data: &[u8]| {
        if key.objectid != EXTENT_CSUM_OBJECTID || key.key_type != EXTENT_CSUM_KEY {
            // Keys ascend, so anything else ends the run of csum items.
            return Ok(false);
        }
        if key.offset >= end {
            return Ok(false);
        }
        for (i, digest) in data.chunks_exact(csum_size).enumerate() {
            let sector = key.offset.saturating_add(i as u64 * sectorsize);
            if sector >= end {
                break;
            }
            if sector >= start {
                out.insert(sector, digest.to_vec());
            }
        }
        Ok(true)
    })?;

    Ok(out)
}
