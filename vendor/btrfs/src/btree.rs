//! B-tree nodes, leaves and traversal.
//!
//! # One structure, every tree
//!
//! Btrfs stores the chunk tree, the root tree, each subvolume's file
//! tree, the extent tree and the checksum tree in exactly the same
//! shape: a copy-on-write B-tree of fixed-size blocks, `nodesize` bytes
//! each, every block opening with the same [`Header`]. A block with
//! `level == 0` is a leaf and holds [`Item`]s; a block above level 0 is
//! an internal node and holds [`KeyPtr`]s naming child blocks. Nothing
//! in this module is specific to any one tree, which is why implementing
//! it once is enough to reach all of them.
//!
//! # Byte order
//!
//! Little-endian throughout, checksum included, like the rest of the
//! format.
//!
//! # Layout
//!
//! ```text
//!  0x00  +--------------------------------+
//!        | header (HEADER_SIZE bytes)     |
//!  0x65  +--------------------------------+  <- LEAF_DATA_OFFSET
//!        | item array / key_ptr array     |  grows forwards
//!        |            ...                 |
//!        |          free space            |
//!        |            ...                 |
//!        | item data, last item first     |  grows backwards
//! nodesize +------------------------------+
//! ```
//!
//! The two ends grow towards each other, so "the item array does not
//! reach the item data" is the leaf's integrity invariant and is checked
//! on every parse.
//!
//! **The `offset` field of an [`Item`] is measured from the end of the
//! header, not from the start of the block.** That single fact is the
//! easiest thing in this structure to get wrong, and getting it wrong
//! shifts every item's data by [`HEADER_SIZE`] bytes — far enough to
//! produce garbage, close enough to look like a plausible parse.
//!
//! # Trust
//!
//! A block is only parsed after it has proved three things about itself:
//! its checksum matches, its `bytenr` is the address the caller asked
//! for, and its `fsid` is this volume's. The checksum catches corrupted
//! bits. The other two catch an *intact* block that came from the wrong
//! place — a stale tree block left behind by an earlier filesystem, or a
//! logical-to-physical translation that quietly went wrong — which no
//! checksum can detect, because such a block's checksum is perfectly
//! good.
//!
//! # Confidence
//!
//! Offsets here were derived from the field order of
//! `struct btrfs_header`, `struct btrfs_key_ptr` and `struct btrfs_item`
//! in the published on-disk format, and every one of them is corroborated
//! by arithmetic that has to close: the three struct sizes named below
//! are the sums of their own field widths, and `HEADER_SIZE` is
//! independently confirmed by the fact that `nodesize - HEADER_SIZE` is
//! the leaf data area every item offset is relative to. Anything that
//! could not be corroborated twice is called out at its use site.

use alloc::{vec::Vec, string::String};
use core::cmp::Ordering;
use core::fmt;

use crate::chunk::{DiskKey, DISK_KEY_SIZE};
use crate::error::{Error, Result};
use crate::superblock::{
    le32, le64, uuid_at, ChecksumType, Superblock, CSUM_SIZE, MAX_LEVEL, UUID_SIZE,
};

/// Size of an on-disk `struct btrfs_header`.
///
/// The field widths sum to 101: csum 32, fsid 16, bytenr 8, flags 8,
/// chunk_tree_uuid 16, generation 8, owner 8, nritems 4, level 1. The
/// struct is packed, so there is no tail padding despite the odd
/// trailing byte.
pub const HEADER_SIZE: usize = 101;

/// Where a leaf's data area begins, and the origin every [`Item::offset`]
/// is measured from.
///
/// Identical to [`HEADER_SIZE`] — the item array and the item data share
/// one coordinate system that starts immediately after the header — but
/// named separately because the two are conceptually different things
/// and conflating them is how the offset origin gets lost.
pub const LEAF_DATA_OFFSET: usize = HEADER_SIZE;

/// Size of an on-disk `struct btrfs_key_ptr`: a 17-byte key, then
/// `blockptr` and `generation`.
pub const KEY_PTR_SIZE: usize = DISK_KEY_SIZE + 8 + 8;

/// Size of an on-disk `struct btrfs_item`: a 17-byte key, then a 32-bit
/// `offset` and a 32-bit `size`.
pub const ITEM_SIZE: usize = DISK_KEY_SIZE + 4 + 4;

/// Byte offsets within a `struct btrfs_header`.
///
/// The first four fields deliberately mirror the superblock's opening
/// layout — csum, fsid, bytenr, flags at the same offsets — which is a
/// useful sanity anchor: [`crate::superblock::offsets::CSUM`],
/// `FSID` and `BYTENR` carry the same values as the constants below.
pub mod header_offsets {
    /// Checksum of bytes `0x20 .. nodesize`.
    pub const CSUM: usize = 0x00;
    /// Filesystem UUID; `metadata_uuid` when that feature is enabled.
    pub const FSID: usize = 0x20;
    /// Logical address this block belongs at.
    pub const BYTENR: usize = 0x30;
    /// Header flags — see [`super::header_flags`].
    pub const FLAGS: usize = 0x38;
    /// UUID of the chunk tree this block's addresses were allocated
    /// under.
    pub const CHUNK_TREE_UUID: usize = 0x40;
    /// Transaction id that wrote this block.
    pub const GENERATION: usize = 0x50;
    /// Objectid of the tree that owns this block.
    pub const OWNER: usize = 0x58;
    /// Number of key pointers (node) or items (leaf) that follow.
    pub const NRITEMS: usize = 0x60;
    /// Height above the leaves; 0 means this block *is* a leaf.
    pub const LEVEL: usize = 0x64;
}

/// Header `flags` bits.
pub mod header_flags {
    /// The block has been written out at least once.
    pub const WRITTEN: u64 = 1 << 0;
    /// The block belongs to a relocation tree.
    pub const RELOC: u64 = 1 << 1;
    /// Bit position of the back-reference revision, which shares the
    /// `flags` word with the bits above and occupies its top byte.
    pub const BACKREF_REV_SHIFT: u32 = 56;
    /// Mask selecting the back-reference revision.
    pub const BACKREF_REV_MASK: u64 = 0xffu64 << BACKREF_REV_SHIFT;
    /// Back-reference revision predating the `MIXED_BACKREF` feature.
    pub const OLD_BACKREF_REV: u8 = 0;
    /// Back-reference revision used by every filesystem with
    /// `MIXED_BACKREF` set, which is every modern one.
    pub const MIXED_BACKREF_REV: u8 = 1;
}

/// Compare two keys the way Btrfs orders them: `objectid`, then
/// `key_type`, then `offset`, each unsigned.
///
/// This ordering is the whole contract of the B-tree — a search that
/// compares in a different order will descend into the wrong subtree and
/// report a perfectly real item as missing.
pub fn compare_keys(a: &DiskKey, b: &DiskKey) -> Ordering {
    a.objectid
        .cmp(&b.objectid)
        .then(a.key_type.cmp(&b.key_type))
        .then(a.offset.cmp(&b.offset))
}

/// Report a tree block as structurally malformed.
///
/// [`Error`] has no `BadTreeBlock` variant, so these go out as
/// [`Error::BadSuperblock`], whose *documented* meaning — "the volume is
/// Btrfs, but a structural field is out of range or internally
/// inconsistent" — fits even though its name does not. Every message is
/// prefixed with the offending block's logical address so a log line is
/// not mistaken for a complaint about the superblock itself. A dedicated
/// variant would be better and is worth adding the next time `error.rs`
/// is open.
fn bad_block(logical: u64, msg: impl fmt::Display) -> Error {
    Error::BadSuperblock(format!("tree block at {logical:#x}: {msg}"))
}

/// Render a UUID as plain hex, for error messages.
fn hex_uuid(u: &[u8; UUID_SIZE]) -> String {
    let mut s = String::with_capacity(UUID_SIZE * 2);
    for b in u {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The `struct btrfs_header` that opens every tree block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The raw 32-byte checksum field, as stored. Only the first
    /// [`ChecksumType::digest_len`] bytes are meaningful.
    pub csum: [u8; CSUM_SIZE],
    /// The filesystem this block belongs to. Holds `metadata_uuid` when
    /// the `METADATA_UUID` feature is set, which is exactly what
    /// [`Superblock::node_uuid`] returns.
    pub fsid: [u8; UUID_SIZE],
    /// The logical address this block belongs at.
    pub bytenr: u64,
    /// Header flags — see [`header_flags`].
    pub flags: u64,
    /// UUID of the chunk tree in force when this block was allocated.
    pub chunk_tree_uuid: [u8; UUID_SIZE],
    /// Transaction id that wrote this block.
    pub generation: u64,
    /// Objectid of the owning tree — 1 for the root tree, 3 for the
    /// chunk tree, 5 for the top-level file tree, and so on. See
    /// [`crate::chunk::objectid`].
    pub owner: u64,
    /// Number of key pointers (node) or items (leaf) in this block.
    pub nritems: u32,
    /// Height above the leaves. 0 means leaf.
    pub level: u8,
}

impl Header {
    /// Decode a header from the first [`HEADER_SIZE`] bytes of `buf`.
    ///
    /// This only decodes. Nothing is trusted until
    /// [`TreeBlock::parse`] has checked the checksum and the identity
    /// fields.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        use header_offsets as o;
        if buf.len() < HEADER_SIZE {
            return Err(Error::BadSuperblock(format!(
                "tree block header needs {HEADER_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(Header {
            csum: buf[o::CSUM..o::CSUM + CSUM_SIZE]
                .try_into()
                .expect("32 bytes"),
            fsid: uuid_at(buf, o::FSID),
            bytenr: le64(buf, o::BYTENR),
            flags: le64(buf, o::FLAGS),
            chunk_tree_uuid: uuid_at(buf, o::CHUNK_TREE_UUID),
            generation: le64(buf, o::GENERATION),
            owner: le64(buf, o::OWNER),
            nritems: le32(buf, o::NRITEMS),
            level: buf[o::LEVEL],
        })
    }

    /// Whether this block is a leaf and therefore holds items rather
    /// than child pointers.
    pub fn is_leaf(&self) -> bool {
        self.level == 0
    }

    /// The back-reference revision packed into the top byte of `flags`.
    pub fn backref_rev(&self) -> u8 {
        ((self.flags & header_flags::BACKREF_REV_MASK) >> header_flags::BACKREF_REV_SHIFT) as u8
    }

    /// Whether the block has been written out at least once.
    pub fn is_written(&self) -> bool {
        self.flags & header_flags::WRITTEN != 0
    }

    /// Whether the block belongs to a relocation tree.
    pub fn is_reloc(&self) -> bool {
        self.flags & header_flags::RELOC != 0
    }
}

/// One `struct btrfs_key_ptr` — an internal node's pointer to a child
/// block.
///
/// `key` is the *smallest* key anywhere in the subtree the child roots,
/// which is what makes the descent rule work: to find a key, take the
/// last child whose key is less than or equal to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPtr {
    /// Smallest key in the child subtree.
    pub key: DiskKey,
    /// Logical address of the child block.
    pub blockptr: u64,
    /// Transaction id the child was written in. Copy-on-write means a
    /// child older than its parent is normal; a child *newer* than the
    /// tree root is not.
    pub generation: u64,
}

impl KeyPtr {
    /// Parse a key pointer from the first [`KEY_PTR_SIZE`] bytes of
    /// `buf`.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < KEY_PTR_SIZE {
            return Err(Error::BadSuperblock(format!(
                "key pointer needs {KEY_PTR_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(KeyPtr {
            key: DiskKey::parse(buf)?,
            // The struct is packed, so both u64s start on odd byte
            // offsets: 17 and 25.
            blockptr: le64(buf, DISK_KEY_SIZE),
            generation: le64(buf, DISK_KEY_SIZE + 8),
        })
    }
}

/// One `struct btrfs_item` — a leaf's index entry for a piece of item
/// data stored elsewhere in the same block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// The key this item is filed under.
    pub key: DiskKey,
    /// Where the item's data starts, **measured from the end of the
    /// header** ([`LEAF_DATA_OFFSET`]), not from the start of the block.
    pub offset: u32,
    /// How many bytes of data the item has.
    pub size: u32,
}

impl Item {
    /// Parse an item from the first [`ITEM_SIZE`] bytes of `buf`.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < ITEM_SIZE {
            return Err(Error::BadSuperblock(format!(
                "leaf item needs {ITEM_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(Item {
            key: DiskKey::parse(buf)?,
            // Packed struct again: these two u32s start at 17 and 21.
            offset: le32(buf, DISK_KEY_SIZE),
            size: le32(buf, DISK_KEY_SIZE + 4),
        })
    }

    /// One past the last byte of this item's data, in the same
    /// coordinate system as [`Item::offset`].
    pub fn data_end(&self) -> usize {
        self.offset as usize + self.size as usize
    }
}

/// A tree block's contents, which depend entirely on its level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// `level > 0` — pointers to child blocks, in ascending key order.
    Node(Vec<KeyPtr>),
    /// `level == 0` — the items themselves, in ascending key order.
    Leaf(Vec<Item>),
}

impl Body {
    /// The child pointers, or `None` if this is a leaf.
    pub fn key_ptrs(&self) -> Option<&[KeyPtr]> {
        match self {
            Body::Node(p) => Some(p),
            Body::Leaf(_) => None,
        }
    }

    /// The items, or `None` if this is an internal node.
    pub fn items(&self) -> Option<&[Item]> {
        match self {
            Body::Leaf(i) => Some(i),
            Body::Node(_) => None,
        }
    }

    /// How many entries the body holds, whichever kind they are.
    pub fn len(&self) -> usize {
        match self {
            Body::Node(p) => p.len(),
            Body::Leaf(i) => i.len(),
        }
    }

    /// Whether the body holds no entries at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Everything needed to validate a tree block, gathered from the
/// superblock once so the walker does not have to carry it around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeGeometry {
    /// Size of every metadata block, in bytes.
    pub nodesize: u32,
    /// Algorithm protecting each block.
    pub csum_type: ChecksumType,
    /// UUID every block header must carry.
    pub fsid: [u8; UUID_SIZE],
}

impl TreeGeometry {
    /// Take the geometry from a parsed superblock.
    ///
    /// The UUID comes from [`Superblock::node_uuid`] rather than
    /// `fsid`: when the `METADATA_UUID` feature is set the volume's
    /// visible UUID and the one stamped into tree blocks are
    /// deliberately different, and it is the latter a header carries.
    pub fn from_superblock(sb: &Superblock) -> Self {
        TreeGeometry {
            nodesize: sb.nodesize,
            csum_type: sb.csum_type,
            fsid: sb.node_uuid(),
        }
    }
}

/// A whole tree block: its header, its parsed body, and the bytes the
/// item data lives in.
///
/// Constructing one is the only way to get a [`Body`], and construction
/// verifies the checksum and both identity fields first, so a
/// `TreeBlock` in hand is a block that has already proved it is intact
/// and that it belongs where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeBlock {
    /// The parsed header.
    pub header: Header,
    /// Child pointers or items, depending on the level.
    pub body: Body,
    /// The raw block, kept because leaf item data is referenced out of
    /// it by offset.
    bytes: Vec<u8>,
}

impl TreeBlock {
    /// Verify and parse a `nodesize`-byte block read from `logical`.
    ///
    /// # Errors
    ///
    /// [`Error::ChecksumMismatch`] if the stored digest disagrees,
    /// [`Error::BlockIdentityMismatch`] if the header's `bytenr` is not
    /// `logical`, and [`Error::BadSuperblock`] — see `bad_block` — if
    /// the fsid is foreign or the block's internal geometry does not
    /// hold up.
    pub fn parse(bytes: Vec<u8>, logical: u64, geom: &TreeGeometry) -> Result<Self> {
        Self::parse_accepting(bytes, logical, geom, &[])
    }

    /// [`TreeBlock::parse`], also accepting a header stamped with any of
    /// `seeds`. A sprouted filesystem's blocks from before it sprouted
    /// are on its seed devices and carry the seed's fsid, which the
    /// kernel's `check_tree_block_fsid` accepts too (#679).
    ///
    /// # Errors
    ///
    /// As [`TreeBlock::parse`]; a foreign fsid is one that is neither
    /// the geometry's nor a seed's.
    pub fn parse_accepting(
        bytes: Vec<u8>,
        logical: u64,
        geom: &TreeGeometry,
        seeds: &[[u8; UUID_SIZE]],
    ) -> Result<Self> {
        let nodesize = geom.nodesize as usize;
        if bytes.len() != nodesize {
            return Err(bad_block(
                logical,
                format!("got {} bytes for a {nodesize}-byte node", bytes.len()),
            ));
        }
        let header = Header::parse(&bytes)?;

        // Checksum first: nothing below is worth reading if the bits are
        // not the bits that were written. The digest covers
        // `CSUM_SIZE .. nodesize` — the whole block minus the checksum
        // field, exactly the same span rule the superblock uses, just
        // over nodesize instead of 4 KiB.
        if !geom
            .csum_type
            .verify(&bytes[CSUM_SIZE..nodesize], &header.csum)
        {
            return Err(Error::ChecksumMismatch {
                what: "tree block",
                offset: logical,
            });
        }

        // Then identity. An intact block from the wrong place has a
        // perfectly good checksum, so these two checks are the only
        // thing standing between a mistranslated logical address and a
        // confidently wrong answer.
        if header.bytenr != logical {
            return Err(Error::BlockIdentityMismatch {
                what: "tree block",
                expected: logical,
                found: header.bytenr,
            });
        }
        if header.fsid != geom.fsid && !seeds.contains(&header.fsid) {
            // Not BlockIdentityMismatch: that variant carries u64
            // addresses, and squeezing a UUID into one would produce an
            // error message that reads like nonsense.
            return Err(bad_block(
                logical,
                format!(
                    "fsid {} is not this volume's {}",
                    hex_uuid(&header.fsid),
                    hex_uuid(&geom.fsid)
                ),
            ));
        }
        if header.level >= MAX_LEVEL {
            return Err(bad_block(
                logical,
                format!(
                    "level {} is at or above the maximum tree height {MAX_LEVEL}",
                    header.level
                ),
            ));
        }

        let nritems = header.nritems as usize;
        let body = if header.is_leaf() {
            Body::Leaf(parse_leaf_items(&bytes, logical, nritems, nodesize)?)
        } else {
            Body::Node(parse_key_ptrs(&bytes, logical, nritems, nodesize)?)
        };

        Ok(TreeBlock {
            header,
            body,
            bytes,
        })
    }

    /// The raw block bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether this block is a leaf.
    pub fn is_leaf(&self) -> bool {
        self.header.is_leaf()
    }

    /// The data belonging to `item`.
    ///
    /// Returns `None` only if `item` did not come from this block:
    /// [`TreeBlock::parse`] bounds-checks every item it returns, so for
    /// those the lookup always succeeds.
    pub fn item_data(&self, item: &Item) -> Option<&[u8]> {
        let start = LEAF_DATA_OFFSET.checked_add(item.offset as usize)?;
        let end = start.checked_add(item.size as usize)?;
        self.bytes.get(start..end)
    }
}

/// Parse and validate a leaf's item array.
///
/// Three separate invariants are checked, and each one is a cheap
/// detector for a different mistake:
///
/// - every item's data lies inside the leaf data area, and starts after
///   the item array — catching a wrong offset origin or a corrupt size;
/// - the items' data runs are exactly contiguous, ending at the block's
///   end and butting up against each other — this is what the kernel
///   enforces on every leaf it reads, and it fails loudly if the item
///   stride or field offsets are wrong;
/// - keys strictly ascend — the B-tree's entire search contract.
fn parse_leaf_items(
    bytes: &[u8],
    logical: u64,
    nritems: usize,
    nodesize: usize,
) -> Result<Vec<Item>> {
    let array_end = nritems
        .checked_mul(ITEM_SIZE)
        .ok_or_else(|| bad_block(logical, format!("nritems {nritems} overflows")))?;
    if LEAF_DATA_OFFSET + array_end > nodesize {
        return Err(bad_block(
            logical,
            format!(
                "{nritems} items need {} bytes but the node is only {nodesize}",
                LEAF_DATA_OFFSET + array_end
            ),
        ));
    }
    let leaf_data_size = nodesize - LEAF_DATA_OFFSET;

    let mut items: Vec<Item> = Vec::with_capacity(nritems);
    // Item 0's data ends at the very end of the block; each subsequent
    // item's data ends where the previous one's began.
    let mut expected_end = leaf_data_size;
    for i in 0..nritems {
        let at = LEAF_DATA_OFFSET + i * ITEM_SIZE;
        let item = Item::parse(&bytes[at..])
            .map_err(|_| bad_block(logical, format!("item {i} is truncated at byte {at}")))?;
        let end = item.data_end();
        if end > leaf_data_size {
            return Err(bad_block(
                logical,
                format!(
                    "item {i} data [{}, {end}) runs past the {leaf_data_size}-byte data area",
                    item.offset
                ),
            ));
        }
        if (item.offset as usize) < array_end {
            return Err(bad_block(
                logical,
                format!(
                    "item {i} data starts at {}, inside the item array that ends at {array_end}",
                    item.offset
                ),
            ));
        }
        if end != expected_end {
            return Err(bad_block(
                logical,
                format!("item {i} data ends at {end}, expected {expected_end} — items are not contiguous"),
            ));
        }
        expected_end = item.offset as usize;
        if let Some(prev) = items.last() {
            if compare_keys(&prev.key, &item.key) != Ordering::Less {
                return Err(bad_block(
                    logical,
                    format!("item {i} key {:?} does not follow {:?}", item.key, prev.key),
                ));
            }
        }
        items.push(item);
    }
    Ok(items)
}

/// Parse and validate an internal node's key-pointer array.
fn parse_key_ptrs(
    bytes: &[u8],
    logical: u64,
    nritems: usize,
    nodesize: usize,
) -> Result<Vec<KeyPtr>> {
    if nritems == 0 {
        // A node with no children cannot be descended and cannot have
        // been produced by a working allocator.
        return Err(bad_block(logical, "internal node has no children"));
    }
    let array_end = nritems
        .checked_mul(KEY_PTR_SIZE)
        .ok_or_else(|| bad_block(logical, format!("nritems {nritems} overflows")))?;
    if HEADER_SIZE + array_end > nodesize {
        return Err(bad_block(
            logical,
            format!(
                "{nritems} key pointers need {} bytes but the node is only {nodesize}",
                HEADER_SIZE + array_end
            ),
        ));
    }

    let mut ptrs: Vec<KeyPtr> = Vec::with_capacity(nritems);
    for i in 0..nritems {
        let at = HEADER_SIZE + i * KEY_PTR_SIZE;
        let ptr = KeyPtr::parse(&bytes[at..]).map_err(|_| {
            bad_block(
                logical,
                format!("key pointer {i} is truncated at byte {at}"),
            )
        })?;
        if ptr.blockptr == 0 {
            return Err(bad_block(logical, format!("child {i} has a null blockptr")));
        }
        if let Some(prev) = ptrs.last() {
            if compare_keys(&prev.key, &ptr.key) != Ordering::Less {
                return Err(bad_block(
                    logical,
                    format!("child {i} key {:?} does not follow {:?}", ptr.key, prev.key),
                ));
            }
        }
        ptrs.push(ptr);
    }
    Ok(ptrs)
}

/// The slot an internal node descends into when looking for `key`: the
/// last child whose key is less than or equal to `key`.
///
/// When `key` sorts before every child, slot 0 is used anyway. That is
/// not a fudge — it is what the reference implementation does, and it is
/// correct, because the leftmost subtree is where such a key would be
/// inserted and therefore the only place it could already be.
fn node_slot(ptrs: &[KeyPtr], key: &DiskKey) -> usize {
    let above = ptrs.partition_point(|p| compare_keys(&p.key, key) != Ordering::Greater);
    above.saturating_sub(1)
}

/// The slot a leaf search settles on: the first item whose key is
/// greater than or equal to `key`. Equal to `items.len()` when `key`
/// sorts after everything in the leaf.
fn leaf_slot(items: &[Item], key: &DiskKey) -> usize {
    items.partition_point(|i| compare_keys(&i.key, key) == Ordering::Less)
}

/// How the walker obtains bytes.
///
/// The caller supplies the device: given a logical address and a buffer
/// exactly `nodesize` long, fill the buffer. Keeping this a plain
/// closure rather than a device trait is deliberate — it lets the whole
/// traversal be exercised against an in-memory byte slice, and it keeps
/// this module from depending on the block-device layer.
/// Look up a block this walker, or another over the same volume, already
/// verified.
pub type CacheGet<'a> = &'a dyn Fn(u64) -> Option<TreeBlock>;
/// Record a block that has just verified.
pub type CachePut<'a> = &'a dyn Fn(u64, &TreeBlock);

pub type ReadBlock<'a> = &'a dyn Fn(u64, &mut [u8]) -> Result<()>;

/// Read copy `mirror` of the block at a logical address.
///
/// Mirror 0 is the copy [`ReadBlock`] returns; the others are the ones
/// DUP and RAID1 keep, and are what a damaged first copy is supposed to
/// be recoverable from.
pub type ReadMirror<'a> = &'a dyn Fn(u64, usize, &mut [u8]) -> Result<()>;

/// How many copies the volume keeps of the block at a logical address.
pub type MirrorCount<'a> = &'a dyn Fn(u64) -> Result<usize>;

/// One item lifted out of a leaf, with its data copied.
///
/// The copy is deliberate: the leaf it came from is a temporary, and an
/// item that borrowed from it would pin a whole `nodesize` block for the
/// sake of a few bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafItem {
    /// The key the item was filed under.
    pub key: DiskKey,
    /// The item's data.
    pub data: Vec<u8>,
}

/// A B-tree, reachable through a caller-supplied block reader.
///
/// One instance can walk any number of trees — the root address is a
/// per-call argument, not state — because every Btrfs tree shares this
/// exact structure.
pub struct Tree<'a> {
    geom: TreeGeometry,
    read: ReadBlock<'a>,
    /// The other copies, where the caller can supply them. `None` means
    /// "one copy, or the caller cannot reach the others", and is what
    /// every in-memory walker uses.
    redundancy: Option<(MirrorCount<'a>, ReadMirror<'a>)>,
    /// Verified blocks kept between walks, where the caller keeps them.
    cache: Option<(CacheGet<'a>, CachePut<'a>)>,
    /// Fsids a block may carry besides the geometry's: a sprouted
    /// filesystem's seeds' (#679). Empty for every other volume.
    seeds: &'a [[u8; UUID_SIZE]],
}

impl<'a> Tree<'a> {
    /// Build a walker over an explicit geometry.
    ///
    /// The walker reads one copy. Use [`Tree::with_redundancy`] to give
    /// it the others.
    pub fn new(geom: TreeGeometry, read: ReadBlock<'a>) -> Self {
        Tree {
            geom,
            read,
            redundancy: None,
            cache: None,
            seeds: &[],
        }
    }

    /// Let the walker fall back to the volume's other copies of a block
    /// when the first one does not verify.
    ///
    /// # WHY THIS IS AN OPT-IN RATHER THAN THE DEFAULT
    ///
    /// A `Tree` is deliberately readable against a plain byte slice --
    /// that is what makes the traversal testable without a block device
    /// -- and a slice has one copy. Requiring a mirror reader
    /// everywhere would push the chunk map into every caller that only
    /// wants to walk some bytes. So the fallback arrives where a
    /// [`crate::chunk::ChunkMap`] already exists, and nowhere else
    /// changes.
    pub fn with_redundancy(mut self, mirrors: MirrorCount<'a>, read: ReadMirror<'a>) -> Self {
        self.redundancy = Some((mirrors, read));
        self
    }

    /// Also accept blocks stamped with any of `seeds`: the fsids of a
    /// sprouted filesystem's seed devices, which hold its blocks from
    /// before it sprouted (#679). See [`TreeBlock::parse_accepting`].
    pub fn with_seed_fsids(mut self, seeds: &'a [[u8; UUID_SIZE]]) -> Self {
        self.seeds = seeds;
        self
    }

    /// Serve blocks from, and record verified blocks in, a cache the
    /// caller keeps across walks (#67). A cached block has already passed
    /// [`TreeBlock::parse`], so a hit skips the read and the checksum; a
    /// parent's generation is still compared against it.
    pub fn with_cache(mut self, get: CacheGet<'a>, put: CachePut<'a>) -> Self {
        self.cache = Some((get, put));
        self
    }

    /// Build a walker using the geometry a parsed superblock implies.
    pub fn from_superblock(sb: &Superblock, read: ReadBlock<'a>) -> Self {
        Tree::new(TreeGeometry::from_superblock(sb), read)
    }

    /// The geometry this walker validates against.
    pub fn geometry(&self) -> &TreeGeometry {
        &self.geom
    }

    /// Read, verify and parse the block at `logical`, trying the
    /// volume's other copies if the first one does not verify.
    ///
    /// # THE COPIES EXIST TO BE READ
    ///
    /// This used to read copy 0 and stop. A tree block whose first copy
    /// failed [`TreeBlock::parse`] -- its own checksum, or an identity
    /// that does not match the address it was fetched from -- made the
    /// whole operation fail, on a volume the kernel mounts without
    /// complaint by reading the second copy. `mkfs.btrfs` picks DUP for
    /// metadata on a single disk in many configurations, so this is the
    /// ordinary case rather than an exotic one, and a driver that
    /// refuses a volume the kernel accepts is hard to tell, from
    /// outside, from a driver that is simply wrong.
    ///
    /// # THE FIRST ERROR IS THE ONE REPORTED
    ///
    /// When no copy verifies, the error returned is copy 0's. A later
    /// mirror can fail for a duller reason -- a short device, an
    /// unmapped address -- and reporting that one would describe the
    /// recovery attempt rather than the damage.
    ///
    /// Verification is [`TreeBlock::parse`] and nothing weaker: a copy
    /// that reads without an I/O error but does not checksum is not a
    /// copy, which is the whole reason a retry can be trusted to have
    /// improved matters.
    ///
    /// # A COPY THAT CANNOT BE READ IS A COPY THAT FAILED
    ///
    /// The primary read was a `?`, so it returned before `redundancy`
    /// was consulted at all: the retry ran only when copy 0 was READ
    /// successfully and then failed to parse. An unreadable sector
    /// under copy 0 -- which is the ordinary way a disk goes bad, and
    /// the case in this function's own title -- still made a DUP or
    /// RAID1 volume unreadable. Inside the loop below the same error
    /// has always been a `continue`; the two halves disagreed about
    /// whether an I/O error is a reason to try the other copy.
    ///
    /// The buffer is deliberately NOT parsed when the read fails.
    /// `parse` on a partly-written or zeroed buffer reports a checksum
    /// mismatch, which would replace the real I/O error with a
    /// description of the wreckage -- and it is copy 0's error that
    /// this function promises to report.
    pub fn read_block(&self, logical: u64) -> Result<TreeBlock> {
        self.read_verified(logical, None)
    }

    /// [`Tree::read_block`] for a block reached through a parent's
    /// `KeyPtr`, which also names the transaction the child was written
    /// in.
    ///
    /// # A BLOCK FROM THE WRONG TIME
    ///
    /// Checksum, `bytenr` and `fsid` catch a damaged block and one from
    /// the wrong place. They all pass for a block that was freed in one
    /// transaction and reused in a later one, then reached through a
    /// stale parent: it is intact, it is at that address, and it belongs
    /// to this volume. Copy-on-write makes that ordinary after a torn
    /// commit or a rollback. The kernel's `verify_parent_transid` compares
    /// the child's header generation with the one its parent recorded,
    /// and so does this (#82). A copy that fails the comparison is a copy
    /// that failed, so the other copies are tried, as the kernel does.
    pub fn read_child(&self, logical: u64, generation: u64) -> Result<TreeBlock> {
        self.read_verified(logical, Some(generation))
    }

    fn read_verified(&self, logical: u64, generation: Option<u64>) -> Result<TreeBlock> {
        if let Some((get, _)) = self.cache {
            if let Some(block) = get(logical) {
                if generation.is_none_or(|want| block.header.generation == want) {
                    return Ok(block);
                }
            }
        }
        let block = self.read_uncached(logical, generation)?;
        if let Some((_, put)) = self.cache {
            put(logical, &block);
        }
        Ok(block)
    }

    fn read_uncached(&self, logical: u64, generation: Option<u64>) -> Result<TreeBlock> {
        let parse = |buf: Vec<u8>| {
            let block = TreeBlock::parse_accepting(buf, logical, &self.geom, self.seeds)?;
            match generation {
                Some(want) if block.header.generation != want => Err(bad_block(
                    logical,
                    format!(
                        "generation {} where the parent said {want} — parent transid verify failed",
                        block.header.generation
                    ),
                )),
                _ => Ok(block),
            }
        };
        let mut buf = vec![0u8; self.geom.nodesize as usize];
        let first = match (self.read)(logical, &mut buf) {
            Ok(()) => match parse(buf) {
                Ok(block) => return Ok(block),
                Err(e) => e,
            },
            Err(e) => e,
        };
        let Some((mirrors, read_mirror)) = self.redundancy else {
            return Err(first);
        };
        // A volume that cannot say how many copies it keeps has told us
        // nothing to try, not that there are none to try -- either way
        // there is nothing further to read.
        let Ok(count) = mirrors(logical) else {
            return Err(first);
        };
        for mirror in 1..count {
            let mut buf = vec![0u8; self.geom.nodesize as usize];
            if read_mirror(logical, mirror, &mut buf).is_err() {
                continue;
            }
            if let Ok(block) = parse(buf) {
                return Ok(block);
            }
        }
        Err(first)
    }

    /// Descend from `root` to the leaf that would hold `key`.
    ///
    /// Returns the leaf whether or not the key is actually present —
    /// the leaf is where the caller looks next, and "not there" is a
    /// property of the leaf's contents, not of the descent.
    ///
    /// # Errors
    ///
    /// [`Error::BadSuperblock`] if the descent runs deeper than
    /// [`MAX_LEVEL`] blocks or a child's level does not sit exactly one
    /// below its parent's, plus anything [`TreeBlock::parse`] can
    /// report for each block along the way.
    pub fn descend(&self, root: u64, key: &DiskKey) -> Result<TreeBlock> {
        let mut path = self.descend_path(root, key)?;
        Ok(path.pop().expect("a descent reads at least the root"))
    }

    /// Every block a descent from `root` towards `key` reads, the root
    /// first and the leaf last. As [`Tree::descend`].
    pub fn descend_path(&self, root: u64, key: &DiskKey) -> Result<Vec<TreeBlock>> {
        let mut path = Vec::new();
        let mut logical = root;
        let mut expected_level: Option<u8> = None;
        let mut expected_generation: Option<u64> = None;
        // A Btrfs tree is at most MAX_LEVEL blocks tall, so this many
        // reads is enough for any well-formed tree and few enough that a
        // cycle in a corrupt one cannot spin forever.
        for _ in 0..MAX_LEVEL {
            let block = self.read_verified(logical, expected_generation)?;
            if let Some(want) = expected_level {
                if block.header.level != want {
                    return Err(bad_block(
                        logical,
                        format!(
                            "level {} where the parent said {want} — the tree is inconsistent",
                            block.header.level
                        ),
                    ));
                }
            }
            let Some(ptrs) = block.body.key_ptrs() else {
                path.push(block);
                return Ok(path);
            };
            let slot = node_slot(ptrs, key);
            expected_level = Some(block.header.level - 1);
            expected_generation = Some(ptrs[slot].generation);
            logical = ptrs[slot].blockptr;
            path.push(block);
        }
        Err(bad_block(
            root,
            format!("descent did not reach a leaf within {MAX_LEVEL} levels"),
        ))
    }

    /// Find the item filed under exactly `key`, if the tree holds one.
    ///
    /// Keys are unique within a tree, so an exact match is at most one
    /// item. Use [`Tree::find_all`] for the far commoner case of wanting
    /// every item sharing an objectid and type.
    pub fn search(&self, root: u64, key: &DiskKey) -> Result<Option<LeafItem>> {
        let leaf = self.descend(root, key)?;
        let items = leaf
            .body
            .items()
            .ok_or_else(|| bad_block(root, "descent ended on an internal node"))?;
        let slot = leaf_slot(items, key);
        let Some(item) = items.get(slot) else {
            return Ok(None);
        };
        if compare_keys(&item.key, key) != Ordering::Equal {
            return Ok(None);
        }
        let data = leaf
            .item_data(item)
            .ok_or_else(|| bad_block(leaf.header.bytenr, "item data is out of bounds"))?;
        Ok(Some(LeafItem {
            key: item.key,
            data: data.to_vec(),
        }))
    }

    /// Every item in the tree sharing `objectid` and `key_type`, in key
    /// order.
    ///
    /// This is the shape almost every real lookup takes: a directory's
    /// entries, a file's extents and an inode's extended attributes are
    /// each a run of items under one objectid and one type, distinguished
    /// only by their key offset.
    pub fn find_all(&self, root: u64, objectid: u64, key_type: u8) -> Result<Vec<LeafItem>> {
        let start = DiskKey {
            objectid,
            key_type,
            offset: 0,
        };
        let mut out = Vec::new();
        self.for_each_from(root, &start, &mut |key, data| {
            if key.objectid != objectid || key.key_type != key_type {
                // Keys ascend, so the first key past the run ends it.
                return Ok(false);
            }
            out.push(LeafItem {
                key: *key,
                data: data.to_vec(),
            });
            Ok(true)
        })?;
        Ok(out)
    }

    /// Visit every item in the tree, in key order.
    ///
    /// `visit` returns `false` to stop early. Item data is passed by
    /// reference straight out of the leaf, so a caller that only needs
    /// to inspect a few bytes never copies the rest.
    pub fn for_each(
        &self,
        root: u64,
        visit: &mut dyn FnMut(&DiskKey, &[u8]) -> Result<bool>,
    ) -> Result<()> {
        self.walk(root, None, &mut WalkState::new(), visit)
            .map(|_| ())
    }

    /// Visit every item from `start` onwards, in key order.
    ///
    /// The tree is descended to `start` rather than scanned from the
    /// beginning, so seeking deep into a large tree costs one block read
    /// per level and nothing more.
    pub fn for_each_from(
        &self,
        root: u64,
        start: &DiskKey,
        visit: &mut dyn FnMut(&DiskKey, &[u8]) -> Result<bool>,
    ) -> Result<()> {
        self.walk(root, Some(start), &mut WalkState::new(), visit)
            .map(|_| ())
    }

    /// Depth-first, left-to-right walk. Returns `false` once `visit` has
    /// asked to stop, so every caller up the recursion unwinds.
    ///
    /// `start` applies only to the leftmost path: once the walk has
    /// moved right of the starting position, every subsequent subtree is
    /// visited whole.
    fn walk(
        &self,
        logical: u64,
        start: Option<&DiskKey>,
        state: &mut WalkState,
        visit: &mut dyn FnMut(&DiskKey, &[u8]) -> Result<bool>,
    ) -> Result<bool> {
        if state.depth >= MAX_LEVEL {
            return Err(bad_block(
                logical,
                format!("walk went deeper than {MAX_LEVEL} levels"),
            ));
        }
        // ONCE EACH. A tree reaches every block by exactly one path, so
        // a block reached twice means the structure is not a tree.
        //
        // `MAX_LEVEL` bounds depth and says nothing about fan-out, and
        // the two are not the same bound at all: eight blocks totalling
        // 32 KiB, each interior one pointing all its slots at the same
        // child a level down, cost 2.4 million reads at a fanout of
        // eight. At the real fanout of a 4 KiB node -- 121 -- that is
        // about 4e14, and at 64 KiB nodes 1e23. Every one of those
        // blocks passes each individual check, because each genuinely
        // is the block at its own address.
        //
        // This is reached three times during mount, before any user
        // call: the chunk tree, `root_item_target`, and `load_fs_tree`.
        if !state.seen.insert(logical) {
            return Err(bad_block(
                logical,
                "block appears twice in one walk — the tree is not a tree",
            ));
        }
        let block = self.read_verified(logical, state.expected_generation)?;
        // The same rule `descend` enforces: a child sits one level below
        // its parent. `walk` did not check it.
        if let Some(want) = state.expected_level {
            if block.header.level != want {
                return Err(bad_block(
                    logical,
                    format!(
                        "level {} where the parent said {want} — the tree is inconsistent",
                        block.header.level
                    ),
                ));
            }
        }
        match &block.body {
            Body::Leaf(items) => {
                let first = start.map_or(0, |k| leaf_slot(items, k));
                for item in items.iter().skip(first) {
                    let data = block.item_data(item).ok_or_else(|| {
                        bad_block(block.header.bytenr, "item data is out of bounds")
                    })?;
                    if !visit(&item.key, data)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Body::Node(ptrs) => {
                let first = start.map_or(0, |k| node_slot(ptrs, k));
                let child_level = block
                    .header
                    .level
                    .checked_sub(1)
                    .ok_or_else(|| bad_block(logical, "a node at level 0 has children"))?;
                for (i, ptr) in ptrs.iter().enumerate().skip(first) {
                    // Only the subtree the start key lands in needs to be
                    // entered partway; everything to its right is whole.
                    let child_start = if i == first { start } else { None };
                    let outer = state.enter(child_level, ptr.generation);
                    let keep_going = self.walk(ptr.blockptr, child_start, state, visit);
                    state.leave(outer);
                    if !keep_going? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
}

/// What one walk has to remember about itself.
///
/// The blocks it has already visited, how deep it is, and what level the
/// block it is about to read should be at. See `Tree::walk`.
struct WalkState {
    seen: alloc::collections::BTreeSet<u64>,
    depth: u8,
    expected_level: Option<u8>,
    expected_generation: Option<u64>,
}

impl WalkState {
    fn new() -> Self {
        Self {
            seen: alloc::collections::BTreeSet::new(),
            depth: 0,
            expected_level: None,
            expected_generation: None,
        }
    }

    /// Step into a child at `level`, written in `generation`, returning
    /// what to restore on the way back out.
    fn enter(&mut self, level: u8, generation: u64) -> (Option<u8>, Option<u64>) {
        let outer = (self.expected_level, self.expected_generation);
        self.expected_level = Some(level);
        self.expected_generation = Some(generation);
        self.depth += 1;
        outer
    }

    fn leave(&mut self, outer: (Option<u8>, Option<u64>)) {
        (self.expected_level, self.expected_generation) = outer;
        self.depth -= 1;
    }
}
