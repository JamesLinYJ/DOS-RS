//! The back references an extent record carries, and what a move does to
//! the ones naming the moved block (#287).
//!
//! An extent item says who refers to the extent. Each reference is either
//! KEYED — "tree 5 refers to this", or for data "inode 588 of tree 5, at
//! file offset 0" — or SHARED, naming the address of the tree block that
//! holds the pointer: its parent. Which one a block's children get is the
//! block's own choice, recorded in its extent item as `FULL_BACKREF`.
//! Balance leaves blocks flagged that way, because it relocates through a
//! tree that is not the one the blocks finally belong to.
//!
//! A copy-on-write move writes a NEW block, owned by the tree it sits in,
//! in a generation no snapshot has seen. Nothing can share that block, so
//! `btrfs check` expects it to be an ordinary one: no `FULL_BACKREF` flag,
//! and its children referred to by the tree rather than by its address.
//! Keeping the flag and re-pointing the shared references at the new
//! address was tried first, and `btrfs check` called it a "bad full
//! backref" and wanted keyed references for every extent under the leaf.
//! So the move converts each shared reference naming the old block into
//! the keyed one the new block's children carry, with the same count:
//! [`take_inline_shared`] takes the shared one out, and the keyed one goes
//! where the kernel would file it, as [`add_inline_keyed`] or
//! [`add_inline_data_ref`] puts it inline, or as an item of its own.
//!
//! References live inline in the extent item when there is room, as a
//! type byte followed by a payload, and otherwise as items of their own
//! keyed `(extent, type, parent)`. Inline ones are ordered by type and,
//! within a type, from the largest offset field down: the kernel's
//! `lookup_inline_extent_backref` scans them in that order, stops at the
//! first one smaller than what it wants, and inserts a new one there. So
//! an edit here takes references out and puts new ones in where that scan
//! would, and never re-sorts the rest. A keyed DATA reference is ordered
//! among its kind by a hash of its fields, which this driver does not
//! compute; a conversion that would have to place one among others is
//! refused by name rather than written in a guessed order.
//!
//! A snapshot shares tree blocks rather than copying them. When a tree
//! copies a block that another tree still reaches, the old block keeps
//! that other tree's reference and becomes `FULL_BACKREF`, and each of its
//! children gains a shared reference naming it -- the old block's own
//! pointers, which the snapshot still reads (#262): [`add_inline_shared`]
//! and [`drop_inline_tree_ref`].

use alloc::{vec::Vec};
use crate::chunk::key_type;
use crate::error::{Error, Result};

/// `BTRFS_BLOCK_FLAG_FULL_BACKREF`: the block's children are referenced
/// by its address rather than by the tree that owns it.
pub const BLOCK_FLAG_FULL_BACKREF: u64 = 1 << 8;

/// `BTRFS_EXTENT_OWNER_REF_KEY`, as an inline reference type.
pub const EXTENT_OWNER_REF: u8 = 172;
/// `BTRFS_TREE_BLOCK_REF_KEY`.
pub const TREE_BLOCK_REF: u8 = crate::extent_write::TREE_BLOCK_REF;
/// `BTRFS_EXTENT_DATA_REF_KEY`.
pub const EXTENT_DATA_REF: u8 = 178;
/// `BTRFS_SHARED_BLOCK_REF_KEY`.
pub const SHARED_BLOCK_REF: u8 = 182;
/// `BTRFS_SHARED_DATA_REF_KEY`.
pub const SHARED_DATA_REF: u8 = 184;

/// The fixed part of an extent item: refs, generation, flags.
const ITEM_HEADER: usize = 24;
/// `btrfs_tree_block_info`: a key and a level, between the flags and the
/// first reference of a tree block's `EXTENT_ITEM` (never of a skinny
/// `METADATA_ITEM`).
const TREE_BLOCK_INFO: usize = 18;
/// `flags`, within the fixed part.
const FLAGS: usize = 16;

/// One inline reference: its type byte and the payload after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRef {
    /// The reference type, one of the `*_REF` constants.
    pub kind: u8,
    /// Everything after the type byte.
    pub payload: Vec<u8>,
}

impl InlineRef {
    /// The first eight payload bytes: the owning tree, the parent, or for
    /// a keyed data reference the tree its inode is in.
    pub fn offset(&self) -> u64 {
        u64::from_le_bytes(
            self.payload[..8]
                .try_into()
                .expect("every payload has 8 bytes"),
        )
    }
}

/// Where the kernel puts a new inline reference of type `kind` and offset
/// field `offset` among `refs`: past every reference of a lower type, and
/// among its own type before the first whose offset is smaller. That is
/// where `lookup_inline_extent_backref`'s scan stops, and where it would
/// look for the reference again. An owner reference, which simple quotas
/// put first, is passed over as the scan passes over it. A keyed data
/// reference is ordered by a hash this does not compute, so the callers
/// place one only where it is the only one of its kind.
fn insert_position(refs: &[InlineRef], kind: u8, offset: u64) -> usize {
    refs.iter()
        .position(|r| {
            r.kind != EXTENT_OWNER_REF && (kind < r.kind || (kind == r.kind && r.offset() < offset))
        })
        .unwrap_or(refs.len())
}

/// How long an inline reference's payload is.
fn payload_len(kind: u8) -> Option<usize> {
    match kind {
        EXTENT_OWNER_REF | TREE_BLOCK_REF | SHARED_BLOCK_REF => Some(8),
        // root, objectid, offset, count
        EXTENT_DATA_REF => Some(28),
        // parent, count
        SHARED_DATA_REF => Some(12),
        _ => None,
    }
}

/// An extent item's flags, or `None` when it is too short to have any.
pub fn flags(body: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(
        body.get(FLAGS..FLAGS + 8)?.try_into().ok()?,
    ))
}

/// Where the inline references start in an item filed under `item_type`.
fn refs_start(item_type: u8, body: &[u8]) -> Result<usize> {
    let flags = flags(body).ok_or_else(|| short(body.len()))?;
    let tree_block = flags & crate::extent_write::EXTENT_FLAG_TREE_BLOCK != 0;
    Ok(if item_type == key_type::EXTENT_ITEM && tree_block {
        ITEM_HEADER + TREE_BLOCK_INFO
    } else {
        ITEM_HEADER
    })
}

fn short(len: usize) -> Error {
    Error::UnsupportedFeature(format!(
        "an extent item of {len} bytes is shorter than its own fixed fields"
    ))
}

/// The inline references of an extent item filed under `item_type`
/// (`EXTENT_ITEM` or `METADATA_ITEM`).
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] for a reference type this does not know
/// the size of, or a body that ends inside a reference: either way the
/// rest of the item cannot be read, and guessing would rewrite it wrong.
pub fn inline_refs(item_type: u8, body: &[u8]) -> Result<Vec<InlineRef>> {
    let mut at = refs_start(item_type, body)?;
    if at > body.len() {
        return Err(short(body.len()));
    }
    let mut out = Vec::new();
    while at < body.len() {
        let kind = body[at];
        let len = payload_len(kind).ok_or_else(|| {
            Error::UnsupportedFeature(format!(
                "an extent item carries an inline reference of type {kind}, whose size \
                 this driver does not know"
            ))
        })?;
        let payload = body.get(at + 1..at + 1 + len).ok_or_else(|| {
            Error::UnsupportedFeature(format!(
                "an extent item ends inside an inline reference of type {kind}"
            ))
        })?;
        out.push(InlineRef {
            kind,
            payload: payload.to_vec(),
        });
        at += 1 + len;
    }
    Ok(out)
}

/// The keyed reference a shared one becomes when the block it names is
/// replaced by an ordinary block of its tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Keyed {
    /// A child tree block: `TREE_BLOCK_REF` naming the tree.
    Block {
        /// The tree the parent belongs to.
        tree: u64,
    },
    /// A data extent: `EXTENT_DATA_REF` naming the tree, the inode and
    /// the file offset of the extent's first byte.
    Data {
        /// The tree the leaf belongs to.
        tree: u64,
        /// The inode whose items point at the extent.
        objectid: u64,
        /// The file offset the extent's first byte would sit at: the
        /// item's key offset less its offset into the extent.
        offset: u64,
    },
}

/// The count a shared reference carries: one for a tree block, the
/// trailing `u32` for data.
fn shared_count(r: &InlineRef) -> u32 {
    if r.kind == SHARED_DATA_REF {
        u32::from_le_bytes(r.payload[8..12].try_into().expect("12-byte payload"))
    } else {
        1
    }
}

/// Rebuild an item body from its fixed part and `refs`, in the order
/// given: each edit keeps the others where they were and puts a new one
/// where the kernel would ([`insert_position`]).
fn rebuild(body: &[u8], start: usize, refs: Vec<InlineRef>) -> Vec<u8> {
    let mut out = body[..start].to_vec();
    for r in &refs {
        out.push(r.kind);
        out.extend_from_slice(&r.payload);
    }
    out
}

/// `body` without its inline shared reference of type `kind` naming
/// `parent`, and that reference's count; `None` when it holds no such
/// reference inline.
///
/// # Errors
///
/// As [`inline_refs`].
pub fn take_inline_shared(
    item_type: u8,
    body: &[u8],
    kind: u8,
    parent: u64,
) -> Result<Option<(Vec<u8>, u32)>> {
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    let Some(at) = refs
        .iter()
        .position(|r| r.kind == kind && r.offset() == parent)
    else {
        return Ok(None);
    };
    let taken = refs.remove(at);
    Ok(Some((rebuild(body, start, refs), shared_count(&taken))))
}

/// `body` with the keyed reference `keyed` added inline, carrying
/// `count`, merged into an identical one when the item already holds it.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] for a tree block the tree already
/// refers to, which would count one reference twice, and for a data
/// reference that would sit beside another keyed data reference: those
/// are ordered by a hash of their fields that this driver does not
/// compute, and a guessed order is one the kernel searches past. And as
/// [`inline_refs`].
pub fn add_inline_keyed(item_type: u8, body: &[u8], keyed: Keyed, count: u32) -> Result<Vec<u8>> {
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    match keyed {
        Keyed::Block { tree } => {
            if refs
                .iter()
                .any(|r| r.kind == TREE_BLOCK_REF && r.offset() == tree)
            {
                return Err(Error::UnsupportedFeature(format!(
                    "the tree block already carries a reference from tree {tree}, so \
                     converting its shared reference would count that tree twice"
                )));
            }
            let at = insert_position(&refs, TREE_BLOCK_REF, tree);
            refs.insert(
                at,
                InlineRef {
                    kind: TREE_BLOCK_REF,
                    payload: tree.to_le_bytes().to_vec(),
                },
            );
        }
        Keyed::Data {
            tree,
            objectid,
            offset,
        } => {
            let mut fields = Vec::with_capacity(24);
            for v in [tree, objectid, offset] {
                fields.extend_from_slice(&v.to_le_bytes());
            }
            let data_refs: Vec<usize> = refs
                .iter()
                .enumerate()
                .filter(|(_, r)| r.kind == EXTENT_DATA_REF)
                .map(|(i, _)| i)
                .collect();
            if let Some(&same) = data_refs
                .iter()
                .find(|&&i| refs[i].payload[..24] == fields[..])
            {
                let had = u32::from_le_bytes(
                    refs[same].payload[24..28]
                        .try_into()
                        .expect("28-byte payload"),
                );
                refs[same].payload[24..28].copy_from_slice(&(had + count).to_le_bytes());
            } else if !data_refs.is_empty() {
                return Err(Error::UnsupportedFeature(format!(
                    "the data extent already carries {} other keyed data reference(s), and \
                     a new one is ordered among them by a hash of its fields that this \
                     driver does not compute",
                    data_refs.len()
                )));
            } else {
                let mut payload = fields;
                payload.extend_from_slice(&count.to_le_bytes());
                // The only one of its kind, so its hash places it nowhere
                // in particular: after the lower types, before the higher.
                let at = insert_position(&refs, EXTENT_DATA_REF, 0);
                refs.insert(
                    at,
                    InlineRef {
                        kind: EXTENT_DATA_REF,
                        payload,
                    },
                );
            }
        }
    }
    Ok(rebuild(body, start, refs))
}

/// `body` with one reference of the inline `EXTENT_DATA_REF` naming
/// `tree`, `objectid` and `offset` dropped (#262): its count down by one,
/// or the reference removed when its count was one, and the item's total
/// down by one. `None` when the item holds no such reference inline.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when the reference is the item's last:
/// dropping it is releasing the extent, which is not this edit. And as
/// [`inline_refs`].
pub fn drop_inline_data_ref(
    item_type: u8,
    body: &[u8],
    tree: u64,
    objectid: u64,
    offset: u64,
) -> Result<Option<Vec<u8>>> {
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    let mut fields = Vec::with_capacity(24);
    for v in [tree, objectid, offset] {
        fields.extend_from_slice(&v.to_le_bytes());
    }
    let Some(at) = refs
        .iter()
        .position(|r| r.kind == EXTENT_DATA_REF && r.payload[..24] == fields[..])
    else {
        return Ok(None);
    };
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    if total <= 1 {
        return Err(Error::UnsupportedFeature(format!(
            "the data reference of inode {objectid} in tree {tree} at {offset} is its extent's \
             last, and dropping it is releasing the extent"
        )));
    }
    let count = u32::from_le_bytes(
        refs[at].payload[24..28]
            .try_into()
            .expect("28-byte payload"),
    );
    if count > 1 {
        refs[at].payload[24..28].copy_from_slice(&(count - 1).to_le_bytes());
    } else {
        refs.remove(at);
    }
    let mut head = body[..start].to_vec();
    head[..8].copy_from_slice(&(total - 1).to_le_bytes());
    Ok(Some(rebuild(&head, start, refs)))
}

/// The body of a keyed `EXTENT_DATA_REF` item -- a data reference the
/// kernel filed as an item of its own once its record had no room inline
/// (#442) -- with one reference dropped: its count down by one, or `None`
/// when its count was one and the item goes.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when the item is not 28 bytes naming
/// `tree`, `objectid` and `offset` (a hash collision files the reference
/// under another key, which is not searched here) or counts nothing.
pub fn drop_keyed_data_ref(
    item: &[u8],
    tree: u64,
    objectid: u64,
    offset: u64,
) -> Result<Option<Vec<u8>>> {
    let mut fields = Vec::with_capacity(24);
    for v in [tree, objectid, offset] {
        fields.extend_from_slice(&v.to_le_bytes());
    }
    if item.len() != 28 || item[..24] != fields[..] {
        return Err(Error::UnsupportedFeature(format!(
            "the keyed data reference where inode {objectid} in tree {tree} at {offset} \
             hashes is {} bytes naming something else",
            item.len()
        )));
    }
    let count = u32::from_le_bytes(item[24..28].try_into().expect("28 bytes"));
    match count {
        0 => Err(Error::UnsupportedFeature(format!(
            "the keyed data reference of inode {objectid} in tree {tree} at {offset} counts \
             nothing"
        ))),
        1 => Ok(None),
        _ => {
            let mut kept = item.to_vec();
            kept[24..28].copy_from_slice(&(count - 1).to_le_bytes());
            Ok(Some(kept))
        }
    }
}

/// An extent record's `body` with one from its total, for a reference
/// dropped from a keyed item rather than inline (#442).
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when the total is one or less: dropping
/// the last reference is releasing the extent, which is not this edit.
/// And as [`refs_start`].
pub fn lower_total(item_type: u8, body: &[u8]) -> Result<Vec<u8>> {
    refs_start(item_type, body)?;
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    if total <= 1 {
        return Err(Error::UnsupportedFeature(
            "the reference is its extent's last, and dropping it is releasing the extent".into(),
        ));
    }
    let mut out = body.to_vec();
    out[..8].copy_from_slice(&(total - 1).to_le_bytes());
    Ok(out)
}

/// An extent record's `body` with `count` off its total, for references
/// taken away by another edit that adds references back or leaves others
/// counted (#734). Unlike [`lower_total`] this can reach zero, which only a
/// reference added straight after it makes right: the caller's render checks
/// the record it leaves.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when the total is less than `count`: the
/// record does not count what it lists. And as [`refs_start`].
pub fn reduce_total(item_type: u8, body: &[u8], count: u64) -> Result<Vec<u8>> {
    refs_start(item_type, body)?;
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    let left = total.checked_sub(count).ok_or_else(|| {
        Error::UnsupportedFeature(format!(
            "the extent's record counts {total} references, and {count} were to come off"
        ))
    })?;
    let mut out = body.to_vec();
    out[..8].copy_from_slice(&left.to_le_bytes());
    Ok(out)
}

/// Whether `body` holds an inline `EXTENT_DATA_REF` from inode `objectid` in
/// `tree` at file offset `offset`: a new one is then counted there, however
/// long the item is (#734).
///
/// # Errors
///
/// As [`inline_refs`].
pub fn holds_inline_data_ref(
    item_type: u8,
    body: &[u8],
    tree: u64,
    objectid: u64,
    offset: u64,
) -> Result<bool> {
    let mut fields = Vec::with_capacity(24);
    for v in [tree, objectid, offset] {
        fields.extend_from_slice(&v.to_le_bytes());
    }
    Ok(inline_refs(item_type, body)?
        .iter()
        .any(|r| r.kind == EXTENT_DATA_REF && r.payload[..24] == fields[..]))
}

/// `body` with one more reference from the file item of inode `objectid`
/// in `tree` that puts the extent's first byte at file offset `offset`
/// (#269): an inline `EXTENT_DATA_REF` already naming those three counted
/// up, or a new one put where the kernel puts it, and the item's total up
/// by one.
///
/// The kernel's place for a new one: past every inline reference of a
/// lower type, and among the data references before the first whose
/// [hash](crate::extent_write::data_ref_hash) is smaller than its own,
/// so they run from the largest hash down. It searches them in that order
/// and stops at the first smaller one, so a reference put anywhere else
/// is one it cannot find when it comes to drop it. Every other reference
/// keeps its place.
///
/// # Errors
///
/// As [`inline_refs`].
pub fn add_inline_data_ref(
    item_type: u8,
    body: &[u8],
    tree: u64,
    objectid: u64,
    offset: u64,
) -> Result<Vec<u8>> {
    use crate::extent_write::data_ref_hash;
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    let mut fields = Vec::with_capacity(28);
    for v in [tree, objectid, offset] {
        fields.extend_from_slice(&v.to_le_bytes());
    }
    let hash_of = |p: &[u8]| {
        let field = |at: usize| u64::from_le_bytes(p[at..at + 8].try_into().expect("8 bytes"));
        data_ref_hash(field(0), field(8), field(16))
    };
    let want = data_ref_hash(tree, objectid, offset);

    let mut at = refs.len();
    let mut same = None;
    for (i, r) in refs.iter().enumerate() {
        if r.kind > EXTENT_DATA_REF {
            at = i;
            break;
        }
        if r.kind < EXTENT_DATA_REF {
            continue;
        }
        if r.payload[..24] == fields[..] {
            same = Some(i);
            break;
        }
        if hash_of(&r.payload) < want {
            at = i;
            break;
        }
    }
    match same {
        Some(i) => {
            let had = u32::from_le_bytes(refs[i].payload[24..28].try_into().expect("28 bytes"));
            let count = had.checked_add(1).ok_or_else(|| {
                Error::UnsupportedFeature(format!(
                    "the data reference of inode {objectid} in tree {tree} at {offset} already \
                     counts {had} items, the most it can"
                ))
            })?;
            refs[i].payload[24..28].copy_from_slice(&count.to_le_bytes());
        }
        None => {
            fields.extend_from_slice(&1u32.to_le_bytes());
            refs.insert(
                at,
                InlineRef {
                    kind: EXTENT_DATA_REF,
                    payload: fields,
                },
            );
        }
    }
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    let total = total.checked_add(1).ok_or_else(|| {
        Error::UnsupportedFeature("the extent's reference count is at its limit".into())
    })?;
    let mut out = body[..start].to_vec();
    out[..8].copy_from_slice(&total.to_le_bytes());
    for r in &refs {
        out.push(r.kind);
        out.extend_from_slice(&r.payload);
    }
    Ok(out)
}

/// `body` with its total up by `count`, and nothing else: a reference filed
/// as an item of its own rather than inline, which is what
/// `insert_inline_extent_backref` does to the record when it answers
/// `-EAGAIN` (#700).
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] for a total at its limit. And as
/// [`refs_start`].
pub fn raise_total(item_type: u8, body: &[u8], count: u64) -> Result<Vec<u8>> {
    refs_start(item_type, body)?;
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    let total = total.checked_add(count).ok_or_else(|| {
        Error::UnsupportedFeature("the extent's reference count is at its limit".into())
    })?;
    let mut out = body.to_vec();
    out[..8].copy_from_slice(&total.to_le_bytes());
    Ok(out)
}

/// `body` with an inline `TREE_BLOCK_REF` naming `tree` added where the
/// kernel's scan would put it, and the item's total up by one (#700): what
/// each child of a copied root gains from `btrfs_inc_ref`.
///
/// # Errors
///
/// As [`add_inline_keyed`], which refuses a tree the block already names.
pub fn add_inline_tree_ref(item_type: u8, body: &[u8], tree: u64) -> Result<Vec<u8>> {
    let added = add_inline_keyed(item_type, body, Keyed::Block { tree }, 1)?;
    raise_total(item_type, &added, 1)
}

/// The body of a keyed `EXTENT_DATA_REF` item naming `tree`, `objectid` and
/// `offset`, counting `count` references (#700).
pub fn keyed_data_ref_body(tree: u64, objectid: u64, offset: u64, count: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(28);
    for v in [tree, objectid, offset] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&count.to_le_bytes());
    out
}

/// `body` with `count` shared references of type `kind` naming `parent`
/// added inline, and the item's total up by `count` (#262): what a child
/// of a block a snapshot shares gains when its tree copies that block. A
/// data extent the block's items point at more than once takes all of them
/// in one `SHARED_DATA_REF`, and one already naming `parent` takes the
/// count. The new reference goes where the kernel's scan would put it.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] for a tree block `parent` already names,
/// which would count one pointer twice, a count other than one for a tree
/// block, and a `kind` that is not a shared reference. And as
/// [`inline_refs`].
pub fn add_inline_shared(
    item_type: u8,
    body: &[u8],
    kind: u8,
    parent: u64,
    count: u32,
) -> Result<Vec<u8>> {
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    let existing = refs
        .iter()
        .position(|r| r.kind == kind && r.offset() == parent);
    match (kind, existing) {
        (SHARED_DATA_REF, Some(at)) => {
            let had = shared_count(&refs[at]);
            refs[at].payload[8..12].copy_from_slice(&(had + count).to_le_bytes());
        }
        (SHARED_DATA_REF, None) => {
            let mut payload = parent.to_le_bytes().to_vec();
            payload.extend_from_slice(&count.to_le_bytes());
            let at = insert_position(&refs, kind, parent);
            refs.insert(at, InlineRef { kind, payload });
        }
        (SHARED_BLOCK_REF, Some(_)) => {
            return Err(Error::UnsupportedFeature(format!(
                "the tree block is already referred to by the block at {parent}, so another \
                 reference from it would count one pointer twice"
            )))
        }
        (SHARED_BLOCK_REF, None) if count == 1 => {
            let at = insert_position(&refs, kind, parent);
            refs.insert(
                at,
                InlineRef {
                    kind,
                    payload: parent.to_le_bytes().to_vec(),
                },
            );
        }
        _ => {
            return Err(Error::UnsupportedFeature(format!(
                "{count} shared reference(s) of type {kind} from the block at {parent} is not \
                 a reference copy-on-write adds"
            )))
        }
    }
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    let mut head = body[..start].to_vec();
    head[..8].copy_from_slice(&(total + u64::from(count)).to_le_bytes());
    Ok(rebuild(&head, start, refs))
}

/// `body` without its inline `TREE_BLOCK_REF` naming `tree`, and the
/// item's total down by one (#262): a tree block that tree has copied, and
/// that another tree still points at. `None` when the item holds no such
/// reference inline.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when the reference is the item's last:
/// dropping it is releasing the block, which is not this edit. And as
/// [`inline_refs`].
pub fn drop_inline_tree_ref(item_type: u8, body: &[u8], tree: u64) -> Result<Option<Vec<u8>>> {
    let start = refs_start(item_type, body)?;
    let mut refs = inline_refs(item_type, body)?;
    let Some(at) = refs
        .iter()
        .position(|r| r.kind == TREE_BLOCK_REF && r.offset() == tree)
    else {
        return Ok(None);
    };
    let total = u64::from_le_bytes(body[..8].try_into().expect("a fixed part"));
    if total <= 1 {
        return Err(Error::UnsupportedFeature(format!(
            "the reference from tree {tree} is its tree block's last, and dropping it is \
             releasing the block"
        )));
    }
    refs.remove(at);
    let mut head = body[..start].to_vec();
    head[..8].copy_from_slice(&(total - 1).to_le_bytes());
    Ok(Some(rebuild(&head, start, refs)))
}

/// `body` with `flags` set besides those it has.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] for a body too short to have flags.
pub fn with_flags(body: &[u8], set: u64) -> Result<Vec<u8>> {
    let had = flags(body).ok_or_else(|| short(body.len()))?;
    let mut out = body.to_vec();
    out[FLAGS..FLAGS + 8].copy_from_slice(&(had | set).to_le_bytes());
    Ok(out)
}
