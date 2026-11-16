//! Adding an item to a leaf, and taking one out.
//!
//! Recording an allocation means putting a `METADATA_ITEM` into the
//! extent tree, and releasing one means taking it out. Both are edits to
//! a leaf's item list, and both have to keep the list sorted — a B-tree
//! search bisects on that order, so an unsorted leaf is not slow, it
//! finds some items and silently misses others.
//!
//! [`crate::tree_write::build_leaf`] turns a list into bytes. This
//! produces the list.
//!
//! # Splitting, and why the boundary is not copied
//!
//! When an item will not fit, the leaf divides in two. Unlike almost
//! everything else in this crate's write path, the boundary is NOT part
//! of the on-disk contract: a checksum computed over the wrong span is
//! a filesystem the kernel rejects, and an item offset measured from the
//! wrong place is a leaf it misreads, but a leaf split at a different
//! position is simply a different, equally valid tree. Any split that
//! leaves both halves ordered, non-empty and inside a block reads back
//! correctly.
//!
//! That distinction matters here because the kernel's own boundary was
//! measured and does not follow a simple rule. Three real splits, read
//! from the live tree either side of the event:
//!
//! ```text
//!   42 items -> 22 | 20     len/2 + 1
//!   32 items -> 17 | 15     len/2 + 1
//!   52 items -> 26 | 26     len/2
//! ```
//!
//! `__btrfs_split_leaf` picks its boundary partly from the slot the new
//! item is going into, and tries to leave room there; it can also push
//! items to a sibling instead of splitting at all. Reproducing that
//! needs the insertion slot and the sibling's state, and the fixtures
//! underdetermine it — two of the three above agree on `len/2 + 1` and
//! the third does not.
//!
//! So this halves the item count and does not claim to be the kernel's
//! choice. What IS claimed, and tested against the kernel's own splits
//! in `tests/split_oracle.rs`, is that the result is a valid division:
//! both halves non-empty, in key order, together holding exactly the
//! input, and each fitting in a block.
//!
//! An earlier version of this note said the policy "is not half",
//! inferred from the fill distribution of a populated filesystem — the
//! median leaf is 91-98% full. That inference was wrong: mostly-full
//! leaves are what halving produces once each half is filled up again
//! by later inserts. Reading a rule off a steady state is what the
//! fixtures replaced.

use alloc::{vec::Vec};
use crate::chunk::DiskKey;
use crate::error::{Error, Result};
use crate::tree_write::{space_needed, LeafItem};

/// An item to place, owning its bytes.
///
/// [`LeafItem`] borrows, which is right for encoding a leaf that already
/// exists and wrong for building one that does not: the new item's bytes
/// have to outlive the list being assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedItem {
    pub key: DiskKey,
    pub data: Vec<u8>,
}

impl OwnedItem {
    /// Borrow as a [`LeafItem`] for encoding.
    pub fn as_leaf_item(&self) -> LeafItem<'_> {
        LeafItem {
            key: self.key,
            data: &self.data,
        }
    }
}

/// Order two keys the way the tree is built on: objectid, then type,
/// then offset.
fn order(a: &DiskKey, b: &DiskKey) -> core::cmp::Ordering {
    (a.objectid, a.key_type, a.offset).cmp(&(b.objectid, b.key_type, b.offset))
}

/// Put `item` into `items`, keeping the order.
///
/// `nodesize` is the block size the result must fit in — the only thing
/// about the filesystem this needs, which is why it is a number rather
/// than a [`crate::superblock::Superblock`].
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] if an item with the same key is already
/// there — two items under one key is not a leaf a search can resolve,
/// and silently replacing one would lose whatever it held — or if the
/// result will not fit in a block.
///
/// Not fitting is this function's boundary, not the crate's:
/// [`insert_or_split`] handles it. `insert` refuses so a caller who
/// meant to write exactly one block finds out rather than getting two.
pub fn insert(nodesize: u32, items: &[OwnedItem], item: OwnedItem) -> Result<Vec<OwnedItem>> {
    let at = match items.binary_search_by(|existing| order(&existing.key, &item.key)) {
        Ok(_) => {
            return Err(Error::UnsupportedFeature(format!(
                "the leaf already holds an item under {:?}; replacing it is a different \
                 operation from inserting",
                item.key
            )))
        }
        Err(at) => at,
    };

    let mut out = Vec::with_capacity(items.len() + 1);
    out.extend_from_slice(&items[..at]);
    out.push(item);
    out.extend_from_slice(&items[at..]);

    let borrowed: Vec<LeafItem> = out.iter().map(|i| i.as_leaf_item()).collect();
    let needed = space_needed(&borrowed);
    let capacity = nodesize as usize;
    if needed > capacity {
        return Err(Error::UnsupportedFeature(format!(
            "the item does not fit: {} items need {needed} bytes and a leaf holds \
             {capacity}. `insert` produces one leaf by definition — use \
             `insert_or_split`, which returns two when the contents no longer fit \
             in one, and add the second to the parent.",
            out.len()
        )));
    }
    Ok(out)
}

/// [`insert`] without the size check: the item in key order, a duplicate
/// key still refused, and the result however large it comes out. For a
/// caller that decides only once every edit is in whether the items take
/// one block or two (#342).
///
/// # Errors
///
/// As [`insert`] for a duplicate key; never for size.
pub fn insert_unbounded(items: &[OwnedItem], item: OwnedItem) -> Result<Vec<OwnedItem>> {
    let at = match items.binary_search_by(|existing| order(&existing.key, &item.key)) {
        Ok(_) => {
            return Err(Error::UnsupportedFeature(format!(
                "the leaf already holds an item under {:?}; replacing it is a different \
                 operation from inserting",
                item.key
            )))
        }
        Err(at) => at,
    };
    let mut out = Vec::with_capacity(items.len() + 1);
    out.extend_from_slice(&items[..at]);
    out.push(item);
    out.extend_from_slice(&items[at..]);
    Ok(out)
}

/// Whether `items` fit in one `nodesize`-byte leaf.
pub fn fit(nodesize: u32, items: &[OwnedItem]) -> bool {
    let borrowed: Vec<LeafItem> = items.iter().map(|i| i.as_leaf_item()).collect();
    space_needed(&borrowed) <= nodesize as usize
}

/// Divide `items` into non-empty leaves that each fit in `nodesize`
/// bytes. Keep the closest-sized pair when two leaves suffice (#342);
/// otherwise fill successive leaves in key order (#615).
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when there are fewer than two items or
/// a single item cannot fit in a leaf; item bodies cannot be divided here.
pub fn split_to_fit(nodesize: u32, items: &[OwnedItem]) -> Result<Vec<Vec<OwnedItem>>> {
    if items.len() < 2 {
        return Err(Error::UnsupportedFeature(format!(
            "a leaf of {} item(s) cannot be split into two that both hold something",
            items.len()
        )));
    }
    let mut best: Option<(usize, usize)> = None;
    for at in 1..items.len() {
        let (left, right) = (&items[..at], &items[at..]);
        if !fit(nodesize, left) || !fit(nodesize, right) {
            continue;
        }
        let size = |half: &[OwnedItem]| {
            let borrowed: Vec<LeafItem> = half.iter().map(|i| i.as_leaf_item()).collect();
            space_needed(&borrowed)
        };
        let gap = size(left).abs_diff(size(right));
        if best.is_none_or(|(_, g)| gap < g) {
            best = Some((at, gap));
        }
    }
    if let Some((at, _)) = best {
        return Ok(vec![items[..at].to_vec(), items[at..].to_vec()]);
    }
    let mut leaves = Vec::new();
    let mut start = 0;
    let mut used = crate::btree::HEADER_SIZE;
    for (at, item) in items.iter().enumerate() {
        let size = crate::btree::ITEM_SIZE + item.data.len();
        if crate::btree::HEADER_SIZE + size > nodesize as usize {
            return Err(Error::UnsupportedFeature(format!(
                "the item under {:?} does not fit in a {nodesize}-byte leaf",
                item.key
            )));
        }
        if used + size > nodesize as usize {
            leaves.push(items[start..at].to_vec());
            start = at;
            used = crate::btree::HEADER_SIZE;
        }
        used += size;
    }
    leaves.push(items[start..].to_vec());
    Ok(leaves)
}

/// Take the item under `key` out.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] if no item has that key. Removing
/// something that is not there is not a no-op: for a caller releasing an
/// extent it means the extent tree does not say what the caller
/// believes, and continuing would leave a block recorded as allocated
/// for ever.
pub fn delete(items: &[OwnedItem], key: &DiskKey) -> Result<Vec<OwnedItem>> {
    let at = items
        .binary_search_by(|existing| order(&existing.key, key))
        .map_err(|_| {
            Error::UnsupportedFeature(format!(
                "the leaf holds no item under {key:?}, so there is nothing to remove and \
                 the caller's picture of the tree is wrong"
            ))
        })?;

    let mut out = Vec::with_capacity(items.len() - 1);
    out.extend_from_slice(&items[..at]);
    out.extend_from_slice(&items[at + 1..]);
    Ok(out)
}

/// Split a list of items in two.
///
/// Returns the left and right halves, divided at half the item count.
///
/// This is NOT the kernel's boundary, and does not try to be — see the
/// module docs. The split point is not part of the on-disk format, so
/// any division that leaves both halves ordered, non-empty and inside a
/// block produces a filesystem that reads correctly.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] if there are fewer than two items:
/// there is no boundary that leaves something on both sides, and a
/// "split" producing an empty leaf is a leaf the tree has no use for.
///
/// It does NOT check that each half fits in a block. A split of a list
/// that was one item too big always does, and a caller assembling
/// something larger is doing something this does not model.
pub fn split(items: &[OwnedItem]) -> Result<(Vec<OwnedItem>, Vec<OwnedItem>)> {
    if items.len() < 2 {
        return Err(Error::UnsupportedFeature(format!(
            "a leaf of {} item(s) cannot be split into two that both hold something",
            items.len()
        )));
    }
    // Half, rounded up so a two-item leaf gives one each rather than
    // an empty half. The kernel's own boundary varies with the
    // insertion slot — 42 items became 22|20 and 52 became 26|26 — and
    // is deliberately not imitated.
    let mid = items.len().div_ceil(2);
    Ok((items[..mid].to_vec(), items[mid..].to_vec()))
}

/// Put `item` in, splitting if it will not fit.
///
/// Returns one list when it fitted and two when the leaf had to split.
/// The caller writes one block or two accordingly, and — when there are
/// two — must add the second to the parent, which is not done here
/// because this does not know what the parent is.
///
/// # Errors
///
/// As [`insert`], except that not fitting is no longer one.
pub fn insert_or_split(
    nodesize: u32,
    items: &[OwnedItem],
    item: OwnedItem,
) -> Result<Vec<Vec<OwnedItem>>> {
    if fits(nodesize, items, &item) {
        return Ok(vec![insert(nodesize, items, item)?]);
    }
    // Ordered first, then divided: the boundary is a position in the
    // final list, not in the one before the insert.
    let at = match items.binary_search_by(|existing| order(&existing.key, &item.key)) {
        Ok(_) => {
            return Err(Error::UnsupportedFeature(format!(
                "the leaf already holds an item under {:?}; replacing it is a different \
                 operation from inserting",
                item.key
            )))
        }
        Err(at) => at,
    };
    let mut all = Vec::with_capacity(items.len() + 1);
    all.extend_from_slice(&items[..at]);
    all.push(item);
    all.extend_from_slice(&items[at..]);

    let (left, right) = split(&all)?;
    Ok(vec![left, right])
}

/// Whether `item` would fit alongside `items`.
///
/// For a caller that would rather ask than handle the error — deciding
/// whether a transaction needs a split before it starts, rather than
/// halfway through.
pub fn fits(nodesize: u32, items: &[OwnedItem], item: &OwnedItem) -> bool {
    let mut borrowed: Vec<LeafItem> = items.iter().map(|i| i.as_leaf_item()).collect();
    borrowed.push(item.as_leaf_item());
    space_needed(&borrowed) <= nodesize as usize
}
