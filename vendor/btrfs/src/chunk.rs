//! Chunk items and the logical-to-physical address map.
//!
//! # Why this module comes first
//!
//! Every pointer in Btrfs — tree roots, tree node children, file extents
//! — is a *logical* address in a single flat 64-bit space that spans the
//! whole filesystem, however many devices it has. Nothing in the volume
//! can be read until that space can be translated to a `(device, byte
//! offset)` pair, and the translation table is itself stored in a tree
//! (the chunk tree) whose root is named by a logical address.
//!
//! Btrfs breaks that circle with the `sys_chunk_array` embedded in the
//! superblock: a plain, unindexed run of `(key, chunk item)` pairs
//! covering every SYSTEM chunk, which between them cover the chunk tree.
//! Load those, and the chunk tree becomes readable; read the chunk tree,
//! and the rest of the filesystem becomes readable. That bootstrap is
//! [`ChunkMap::from_sys_chunk_array`].
//!
//! # Byte order
//!
//! Little-endian throughout, like the rest of the format.
//!
//! # Address arithmetic
//!
//! A chunk maps a contiguous logical range onto one or more *stripes*,
//! each a contiguous byte range on one device. How an offset within the
//! chunk selects a stripe depends on the chunk's RAID profile:
//!
//! - SINGLE — one stripe; the offset passes straight through.
//! - DUP, RAID1, RAID1C3, RAID1C4 — every stripe is a full copy of the
//!   whole chunk, so the offset passes straight through and the stripe
//!   index selects *which mirror* to read. DUP puts its copies on the
//!   same device; the RAID1 family puts them on different ones.
//! - RAID0 — the chunk is cut into `stripe_len` rows dealt round-robin
//!   across the stripes.
//! - RAID10 — RAID0 across `num_stripes / sub_stripes` groups, with each
//!   group mirrored `sub_stripes` ways.
//!
//! - RAID5 and RAID6 — `num_stripes - 1` or `- 2` data elements per full
//!   stripe, with parity rotating one stripe per full stripe; see
//!   [`crate::raid56`]. The direct read maps like RAID0 over the data
//!   elements. There is one copy, so [`Chunk::num_mirrors`] is 1, and the
//!   further READ ATTEMPTS ([`Chunk::read_attempts`]) rebuild the element
//!   from parity through [`Chunk::parity_read`] rather than map anywhere.

use alloc::{vec::Vec, string::String};
use crate::error::{Error, Result};
use crate::superblock::{le16, le32, le64, uuid_at, Superblock, UUID_SIZE};

/// Size of an on-disk `struct btrfs_disk_key`: `u64` objectid, `u8` type,
/// `u64` offset, packed with no padding.
pub const DISK_KEY_SIZE: usize = 17;

/// Size of the fixed part of a `struct btrfs_chunk`, before its stripe
/// array.
pub const CHUNK_HEADER_SIZE: usize = 48;

/// Size of one on-disk `struct btrfs_stripe`.
pub const STRIPE_SIZE: usize = 32;

/// The stripe length every current `mkfs.btrfs` uses (`BTRFS_STRIPE_LEN`).
///
/// The value is read from each chunk item rather than assumed, but it is
/// worth naming: modern kernels treat 64 KiB as a constant and will
/// complain about anything else.
pub const DEFAULT_STRIPE_LEN: u64 = 64 * 1024;

/// Well-known key type numbers.
pub mod key_type {
    /// `BTRFS_EXTENT_ITEM_KEY` — an allocated run. Its key's `offset`
    /// is the run's length in bytes.
    pub const EXTENT_ITEM: u8 = 168;
    /// `BTRFS_METADATA_ITEM_KEY` — an allocated tree block, under the
    /// `SKINNY_METADATA` feature. Its key's `offset` is the block's
    /// LEVEL, not a length: the length is always `nodesize`. Reading it
    /// as a length is the mistake the feature invites, and it produces
    /// extents of length 0, 1 and 2.
    pub const METADATA_ITEM: u8 = 169;
    /// `BTRFS_DEV_ITEM_KEY`.
    pub const DEV_ITEM: u8 = 216;
    /// `BTRFS_CHUNK_ITEM_KEY`.
    pub const CHUNK_ITEM: u8 = 228;
    /// `BTRFS_BLOCK_GROUP_ITEM_KEY` — one allocation region. Filed
    /// under the region's first byte, with its length as the `offset`.
    pub const BLOCK_GROUP_ITEM: u8 = 192;
    /// `BTRFS_FREE_SPACE_INFO_KEY` — how a block group's free space is
    /// recorded, extents or a bitmap.
    pub const FREE_SPACE_INFO: u8 = 198;
    /// `BTRFS_FREE_SPACE_EXTENT_KEY` — one free run. The item has no
    /// body; the key is the whole of it.
    pub const FREE_SPACE_EXTENT: u8 = 199;
    /// `BTRFS_FREE_SPACE_BITMAP_KEY` — free space as one bit per
    /// sector.
    pub const FREE_SPACE_BITMAP: u8 = 200;
}

/// Well-known objectid numbers.
pub mod objectid {
    /// `BTRFS_DEV_ITEMS_OBJECTID` — the objectid device items are filed
    /// under. Shares the numeric value 1 with the root tree's objectid;
    /// they live in different trees, so there is no ambiguity.
    pub const DEV_ITEMS: u64 = 1;
    /// `BTRFS_ROOT_TREE_OBJECTID`.
    pub const ROOT_TREE: u64 = 1;
    /// `BTRFS_EXTENT_TREE_OBJECTID`.
    pub const EXTENT_TREE: u64 = 2;
    /// `BTRFS_CHUNK_TREE_OBJECTID`.
    pub const CHUNK_TREE: u64 = 3;
    /// `BTRFS_DEV_TREE_OBJECTID`.
    pub const DEV_TREE: u64 = 4;
    /// `BTRFS_FS_TREE_OBJECTID`.
    pub const FS_TREE: u64 = 5;
    /// `BTRFS_FREE_SPACE_TREE_OBJECTID` — the tree recording what is
    /// free, as opposed to the extent tree recording what is not.
    pub const FREE_SPACE_TREE: u64 = 10;
    /// `BTRFS_BLOCK_GROUP_TREE_OBJECTID` — where the block group items
    /// are on a volume with the `block_group_tree` feature, instead of
    /// the extent tree.
    pub const BLOCK_GROUP_TREE: u64 = 11;
    /// `BTRFS_FIRST_CHUNK_TREE_OBJECTID` — the objectid every chunk item
    /// is filed under.
    pub const FIRST_CHUNK_TREE: u64 = 256;
}

/// Chunk / block-group type bits, as stored in a chunk item's `type`
/// field.
pub mod block_group {
    /// Chunk holds file data.
    pub const DATA: u64 = 1 << 0;
    /// Chunk holds system metadata — the chunk tree itself.
    pub const SYSTEM: u64 = 1 << 1;
    /// Chunk holds filesystem metadata.
    pub const METADATA: u64 = 1 << 2;
    /// Striped with no redundancy.
    pub const RAID0: u64 = 1 << 3;
    /// Two-way mirror across devices.
    pub const RAID1: u64 = 1 << 4;
    /// Two copies on the same device.
    pub const DUP: u64 = 1 << 5;
    /// Striped mirrors.
    pub const RAID10: u64 = 1 << 6;
    /// Single-parity stripe set.
    pub const RAID5: u64 = 1 << 7;
    /// Double-parity stripe set.
    pub const RAID6: u64 = 1 << 8;
    /// Three-way mirror across devices.
    pub const RAID1C3: u64 = 1 << 9;
    /// Four-way mirror across devices.
    pub const RAID1C4: u64 = 1 << 10;

    /// Every bit that names a redundancy profile. A chunk may set at most
    /// one of them; setting none means SINGLE.
    pub const PROFILE_MASK: u64 = RAID0 | RAID1 | RAID1C3 | RAID1C4 | RAID5 | RAID6 | DUP | RAID10;

    /// Every bit that names what a chunk holds. A chunk must set at least
    /// one.
    pub const TYPE_MASK: u64 = DATA | SYSTEM | METADATA;
}

/// The redundancy layout of a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkProfile {
    /// One stripe, no redundancy.
    Single,
    /// Two copies on the same device.
    Dup,
    /// Striped across all stripes, no redundancy.
    Raid0,
    /// Two-way mirror.
    Raid1,
    /// Three-way mirror.
    Raid1c3,
    /// Four-way mirror.
    Raid1c4,
    /// Striped mirrors.
    Raid10,
    /// Single parity, rotating.
    Raid5,
    /// Double parity (P and Q), rotating.
    Raid6,
}

impl ChunkProfile {
    /// Derive the profile from a chunk item's `type` field.
    ///
    /// # Errors
    ///
    /// [`Error::BadChunkItem`] if more than one profile bit is set, or if
    /// the chunk does not say what it holds.
    pub fn from_type(chunk_type: u64) -> Result<Self> {
        if chunk_type & block_group::TYPE_MASK == 0 {
            return Err(Error::BadChunkItem(format!(
                "type {chunk_type:#x} names neither DATA, METADATA nor SYSTEM"
            )));
        }
        let profile = chunk_type & block_group::PROFILE_MASK;
        if profile.count_ones() > 1 {
            return Err(Error::BadChunkItem(format!(
                "type {chunk_type:#x} sets {} profile bits at once",
                profile.count_ones()
            )));
        }
        Ok(match profile {
            0 => ChunkProfile::Single,
            block_group::DUP => ChunkProfile::Dup,
            block_group::RAID0 => ChunkProfile::Raid0,
            block_group::RAID1 => ChunkProfile::Raid1,
            block_group::RAID1C3 => ChunkProfile::Raid1c3,
            block_group::RAID1C4 => ChunkProfile::Raid1c4,
            block_group::RAID10 => ChunkProfile::Raid10,
            block_group::RAID5 => ChunkProfile::Raid5,
            block_group::RAID6 => ChunkProfile::Raid6,
            other => {
                return Err(Error::BadChunkItem(format!(
                    "type {chunk_type:#x} has unrecognised profile bit {other:#x}"
                )))
            }
        })
    }

    /// Human-readable profile name, matching the names `mkfs.btrfs` and
    /// `btrfs filesystem df` use.
    pub fn name(self) -> &'static str {
        match self {
            ChunkProfile::Single => "single",
            ChunkProfile::Dup => "dup",
            ChunkProfile::Raid0 => "raid0",
            ChunkProfile::Raid1 => "raid1",
            ChunkProfile::Raid1c3 => "raid1c3",
            ChunkProfile::Raid1c4 => "raid1c4",
            ChunkProfile::Raid10 => "raid10",
            ChunkProfile::Raid5 => "raid5",
            ChunkProfile::Raid6 => "raid6",
        }
    }

    /// Whether every stripe is a full copy of the chunk, so an offset
    /// within the chunk maps through unchanged and the stripe index picks
    /// a mirror.
    pub fn is_mirrored(self) -> bool {
        matches!(
            self,
            ChunkProfile::Dup | ChunkProfile::Raid1 | ChunkProfile::Raid1c3 | ChunkProfile::Raid1c4
        )
    }

    /// Whether the chunk is cut into rows dealt across its stripes.
    pub fn is_striped(self) -> bool {
        matches!(self, ChunkProfile::Raid0 | ChunkProfile::Raid10)
    }
}

/// A `struct btrfs_disk_key` — the 17-byte sort key every tree item and
/// every `sys_chunk_array` entry is filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskKey {
    /// What the item is about.
    pub objectid: u64,
    /// What kind of item it is — see [`key_type`].
    pub key_type: u8,
    /// Type-specific qualifier. For a chunk item this is the logical
    /// address the chunk starts at.
    pub offset: u64,
}

impl DiskKey {
    /// Parse a key from the first [`DISK_KEY_SIZE`] bytes of `buf`.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < DISK_KEY_SIZE {
            return Err(Error::BadChunkItem(format!(
                "key needs {DISK_KEY_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(DiskKey {
            // The key is a packed 17-byte record with no padding, so
            // `offset` starts at an unaligned byte 9. Reading it as if
            // the struct were aligned would skip three bytes into the
            // wrong field.
            objectid: le64(buf, 0),
            key_type: buf[8],
            // Deliberately unaligned: the on-disk struct is packed, so
            // this `u64` starts on byte 9.
            offset: le64(buf, 9),
        })
    }
}

/// One `struct btrfs_stripe` — a contiguous byte range on one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stripe {
    /// Which device this stripe lives on.
    pub devid: u64,
    /// Byte offset of the stripe on that device.
    pub offset: u64,
    /// UUID of the device, cross-checked against the device's own item.
    pub dev_uuid: [u8; UUID_SIZE],
}

impl Stripe {
    /// Parse a stripe from the first [`STRIPE_SIZE`] bytes of `buf`.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < STRIPE_SIZE {
            return Err(Error::BadChunkItem(format!(
                "stripe needs {STRIPE_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(Stripe {
            devid: le64(buf, 0),
            offset: le64(buf, 8),
            dev_uuid: uuid_at(buf, 16),
        })
    }
}

/// A parsed `struct btrfs_chunk` together with the logical address it was
/// filed under.
///
/// The logical start address is *not* stored in the chunk item itself —
/// it is the `offset` of the key the item is filed under. That split is
/// easy to miss and produces an address map that is wrong by exactly one
/// chunk's worth of offset, so [`Chunk::logical`] is carried explicitly
/// rather than being left implicit in the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// First logical address this chunk covers, from its key.
    pub logical: u64,
    /// Length of the logical range in bytes.
    pub length: u64,
    /// Objectid of the tree that owns this chunk.
    pub owner: u64,
    /// Row size for striped profiles, in bytes.
    pub stripe_len: u64,
    /// Raw `type` field — see [`block_group`].
    pub chunk_type: u64,
    /// Optimal I/O alignment hint.
    pub io_align: u32,
    /// Optimal I/O width hint.
    pub io_width: u32,
    /// Minimal I/O size hint.
    pub sector_size: u32,
    /// Number of stripes that follow.
    pub num_stripes: u16,
    /// Mirrors per striped group, meaningful for RAID10.
    pub sub_stripes: u16,
    /// The stripe array.
    pub stripes: Vec<Stripe>,
}

/// The result of translating one logical address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    /// Device to read from.
    pub devid: u64,
    /// Byte offset on that device.
    pub physical: u64,
    /// How many bytes remain contiguous from [`Self::physical`]. A read
    /// longer than this must be split and re-mapped, because the next
    /// byte of the logical range lives on a different stripe or past the
    /// end of the chunk.
    pub len: u64,
}

impl Chunk {
    /// Parse a chunk item filed under logical address `logical`.
    ///
    /// `buf` must start at the chunk item and may extend past it; the
    /// number of bytes consumed is [`Chunk::encoded_len`].
    ///
    /// # Errors
    ///
    /// [`Error::BadChunkItem`] if the buffer is too short for the stripe
    /// count the header declares, if the geometry is degenerate, or if
    /// the stripe count disagrees with the RAID profile.
    pub fn parse(logical: u64, buf: &[u8]) -> Result<Self> {
        if buf.len() < CHUNK_HEADER_SIZE {
            return Err(Error::BadChunkItem(format!(
                "chunk header needs {CHUNK_HEADER_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        let num_stripes = le16(buf, 0x2c);
        if num_stripes == 0 {
            return Err(Error::BadChunkItem("chunk declares zero stripes".into()));
        }
        let need = CHUNK_HEADER_SIZE + usize::from(num_stripes) * STRIPE_SIZE;
        if buf.len() < need {
            return Err(Error::BadChunkItem(format!(
                "chunk with {num_stripes} stripes needs {need} bytes, got {}",
                buf.len()
            )));
        }

        let mut stripes = Vec::with_capacity(usize::from(num_stripes));
        for raw in buf[CHUNK_HEADER_SIZE..need].chunks_exact(STRIPE_SIZE) {
            stripes.push(Stripe::parse(raw)?);
        }

        let chunk = Chunk {
            logical,
            length: le64(buf, 0x00),
            owner: le64(buf, 0x08),
            stripe_len: le64(buf, 0x10),
            chunk_type: le64(buf, 0x18),
            io_align: le32(buf, 0x20),
            io_width: le32(buf, 0x24),
            sector_size: le32(buf, 0x28),
            num_stripes,
            sub_stripes: le16(buf, 0x2e),
            stripes,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    /// How many bytes this chunk item occupies on disk.
    pub fn encoded_len(&self) -> usize {
        CHUNK_HEADER_SIZE + usize::from(self.num_stripes) * STRIPE_SIZE
    }

    /// One past the last logical address this chunk covers.
    pub fn logical_end(&self) -> u64 {
        self.logical.saturating_add(self.length)
    }

    /// The chunk's redundancy profile.
    pub fn profile(&self) -> Result<ChunkProfile> {
        ChunkProfile::from_type(self.chunk_type)
    }

    /// Whether this chunk holds system metadata (the chunk tree).
    pub fn is_system(&self) -> bool {
        self.chunk_type & block_group::SYSTEM != 0
    }

    /// Whether this chunk holds filesystem metadata.
    pub fn is_metadata(&self) -> bool {
        self.chunk_type & block_group::METADATA != 0
    }

    /// Whether this chunk holds file data.
    pub fn is_data(&self) -> bool {
        self.chunk_type & block_group::DATA != 0
    }

    /// How many independent copies of any given byte this chunk holds.
    /// Mirror indices passed to [`Chunk::map_mirror`] run `0 ..
    /// num_mirrors()`.
    pub fn num_mirrors(&self) -> usize {
        match self.profile() {
            Ok(ChunkProfile::Dup | ChunkProfile::Raid1) => 2,
            Ok(ChunkProfile::Raid1c3) => 3,
            Ok(ChunkProfile::Raid1c4) => 4,
            Ok(ChunkProfile::Raid10) => usize::from(self.sub_stripes),
            _ => 1,
        }
    }

    /// How many ways there are to read any given byte of this chunk: its
    /// copies, and for RAID5/6 the parity rebuilds after the one copy.
    ///
    /// Attempt indices run `0 .. read_attempts()`. Writes use
    /// [`Chunk::num_mirrors`], which counts copies only.
    pub fn read_attempts(&self) -> usize {
        match self.parity_count() {
            Some(parity) => crate::raid56::Rebuild::attempts(
                usize::from(self.num_stripes).saturating_sub(parity),
                parity == 2,
            ),
            None => self.num_mirrors(),
        }
    }

    /// The number of parity elements per full stripe, for RAID5 and
    /// RAID6, and `None` for every other profile.
    pub fn parity_count(&self) -> Option<usize> {
        match self.profile() {
            Ok(ChunkProfile::Raid5) => Some(1),
            Ok(ChunkProfile::Raid6) => Some(2),
            _ => None,
        }
    }

    /// Everything a parity rebuild of `logical` reads: where each element
    /// of its full stripe is, which data element `logical` is in, and how
    /// many bytes from `logical` stay inside that element.
    ///
    /// # Errors
    ///
    /// [`Error::UnmappedLogical`] outside the chunk and
    /// [`Error::UnsupportedProfile`] for a chunk without parity.
    pub fn parity_read(&self, logical: u64) -> Result<ParityRead> {
        if logical < self.logical || logical >= self.logical_end() {
            return Err(Error::UnmappedLogical(logical));
        }
        let parity = self.parity_count().ok_or_else(|| {
            Error::UnsupportedProfile("a parity rebuild of a chunk without parity".into())
        })?;
        let n = usize::from(self.num_stripes);
        let k = (n - parity) as u64;
        let offset = logical - self.logical;
        let element = offset / self.stripe_len;
        let within = offset % self.stripe_len;
        let full = element / k;
        let place = crate::raid56::placement(n, parity, full)?;
        let overflow =
            || Error::BadChunkItem("stripe address arithmetic overflowed 64 bits".into());
        let row = full
            .checked_mul(self.stripe_len)
            .and_then(|r| r.checked_add(within))
            .ok_or_else(overflow)?;
        let at = |index: usize| -> Result<(u64, u64)> {
            let stripe = &self.stripes[index];
            Ok((
                stripe.devid,
                stripe.offset.checked_add(row).ok_or_else(overflow)?,
            ))
        };
        Ok(ParityRead {
            want: (element % k) as usize,
            data: place
                .data_stripes
                .iter()
                .map(|&i| at(i))
                .collect::<Result<_>>()?,
            p: at(place.p)?,
            q: place.q.map(at).transpose()?,
            len: (self.stripe_len - within).min(self.logical_end() - logical),
        })
    }

    /// Structural checks that do not need any outside context.
    fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::BadChunkItem(m));

        if self.length == 0 {
            return bad("chunk covers zero logical bytes".into());
        }
        if self.stripe_len == 0 || !self.stripe_len.is_power_of_two() {
            return bad(format!(
                "stripe_len {} is not a non-zero power of two",
                self.stripe_len
            ));
        }
        if self.logical.checked_add(self.length).is_none() {
            return bad(format!(
                "logical range {:#x}+{:#x} overflows the address space",
                self.logical, self.length
            ));
        }

        let profile = self.profile()?;
        let n = self.num_stripes;
        // Stripe counts are fixed by the profile for everything except
        // RAID0 and the parity profiles. Checking them is cheap and
        // catches a stripe array read at the wrong offset, which would
        // otherwise produce a map that is subtly wrong rather than
        // obviously broken.
        let expect_exact = match profile {
            ChunkProfile::Single => Some(1),
            ChunkProfile::Dup | ChunkProfile::Raid1 => Some(2),
            ChunkProfile::Raid1c3 => Some(3),
            ChunkProfile::Raid1c4 => Some(4),
            _ => None,
        };
        if let Some(want) = expect_exact {
            if n != want {
                return bad(format!(
                    "{} chunk has {n} stripes, expected {want}",
                    profile.name()
                ));
            }
        }
        // Same shape as `expect_exact` above, and deliberately so: these
        // profiles constrain the stripe count from below rather than
        // pinning it. Written as a table because the alternative clippy
        // suggests — a `ChunkProfile::Raid0 if n < 2 =>` match guard —
        // makes the arm match only the *failing* case, leaving a valid
        // raid0 to fall through to the catch-all. That reads as though
        // raid0 were unvalidated.
        let expect_at_least = match profile {
            ChunkProfile::Raid0 | ChunkProfile::Raid5 => Some(2),
            ChunkProfile::Raid6 => Some(3),
            _ => None,
        };
        if let Some(least) = expect_at_least {
            if n < least {
                return bad(format!(
                    "{} chunk has {n} stripes, expected at least {least}",
                    profile.name()
                ));
            }
        }
        if profile == ChunkProfile::Raid10 {
            // Modern mkfs always writes sub_stripes = 2 for RAID10,
            // and the kernel's own validator rejects anything else.
            // Flagged for cross-validation: if a real image ever
            // shows a different value, relax this to "non-zero and
            // divides num_stripes".
            if self.sub_stripes != 2 {
                return bad(format!(
                    "raid10 chunk has sub_stripes {}, expected 2",
                    self.sub_stripes
                ));
            }
            if n < 2 || !n.is_multiple_of(self.sub_stripes) {
                return bad(format!(
                    "raid10 chunk has {n} stripes, which is not a positive multiple of sub_stripes {}",
                    self.sub_stripes
                ));
            }
        }
        Ok(())
    }

    /// Checks that need the superblock's geometry.
    ///
    /// Kept separate from `Chunk::validate` so a chunk item can be
    /// parsed and inspected — by a repair tool, say — without a
    /// superblock in hand.
    pub fn validate_geometry(&self, sectorsize: u32) -> Result<()> {
        let s = u64::from(sectorsize);
        if s == 0 {
            return Err(Error::BadChunkItem("sectorsize is zero".into()));
        }
        if !self.logical.is_multiple_of(s) {
            return Err(Error::BadChunkItem(format!(
                "chunk logical start {:#x} is not sectorsize-aligned ({sectorsize})",
                self.logical
            )));
        }
        if !self.length.is_multiple_of(s) {
            return Err(Error::BadChunkItem(format!(
                "chunk length {:#x} is not a multiple of sectorsize {sectorsize}",
                self.length
            )));
        }
        for (i, stripe) in self.stripes.iter().enumerate() {
            if !stripe.offset.is_multiple_of(s) {
                return Err(Error::BadChunkItem(format!(
                    "stripe {i} physical offset {:#x} is not sectorsize-aligned ({sectorsize})",
                    stripe.offset
                )));
            }
        }
        Ok(())
    }

    /// Translate `logical` using the first copy.
    pub fn map(&self, logical: u64) -> Result<Mapping> {
        self.map_mirror(logical, 0)
    }

    /// Translate `logical` using copy `mirror`.
    ///
    /// # Errors
    ///
    /// [`Error::UnmappedLogical`] if the address is outside this chunk,
    /// [`Error::UnsupportedProfile`] for RAID5/RAID6, and
    /// [`Error::BadChunkItem`] if `mirror` names a copy that does not
    /// exist or the stripe arithmetic overflows.
    pub fn map_mirror(&self, logical: u64, mirror: usize) -> Result<Mapping> {
        if logical < self.logical || logical >= self.logical_end() {
            return Err(Error::UnmappedLogical(logical));
        }
        let profile = self.profile()?;
        if matches!(profile, ChunkProfile::Raid5 | ChunkProfile::Raid6) && mirror > 0 {
            return Err(Error::UnsupportedProfile(format!(
                "a {} chunk has one copy; read attempt {mirror} is a rebuild from parity, \
                 which `parity_read` describes and no single mapping can",
                profile.name()
            )));
        }
        let mirrors = self.num_mirrors();
        if mirror >= mirrors {
            return Err(Error::BadChunkItem(format!(
                "mirror {mirror} requested from a {} chunk with {mirrors} copies",
                profile.name()
            )));
        }

        let offset = logical - self.logical;
        let stripe_nr = offset / self.stripe_len;
        let stripe_offset = offset % self.stripe_len;
        let n = u64::from(self.num_stripes);
        let m = mirror as u64;

        // `index` selects the stripe; `row` is how many full rows of
        // stripe_len precede this one *within that stripe*.
        let (index, row) = match profile {
            ChunkProfile::Single => (0, stripe_nr),
            ChunkProfile::Dup
            | ChunkProfile::Raid1
            | ChunkProfile::Raid1c3
            | ChunkProfile::Raid1c4 => (m, stripe_nr),
            ChunkProfile::Raid0 => (stripe_nr % n, stripe_nr / n),
            ChunkProfile::Raid10 => {
                let sub = u64::from(self.sub_stripes);
                let groups = n / sub;
                ((stripe_nr % groups) * sub + m, stripe_nr / groups)
            }
            ChunkProfile::Raid5 | ChunkProfile::Raid6 => {
                let parity = self.parity_count().expect("a parity profile");
                let k = n - parity as u64;
                let full = stripe_nr / k;
                let place = crate::raid56::placement(usize::from(self.num_stripes), parity, full)?;
                (place.data_stripes[(stripe_nr % k) as usize] as u64, full)
            }
        };

        let stripe = self
            .stripes
            .get(index as usize)
            .ok_or_else(|| Error::BadChunkItem(format!("stripe index {index} out of range")))?;

        let overflow =
            || Error::BadChunkItem("stripe address arithmetic overflowed 64 bits".into());
        let physical = row
            .checked_mul(self.stripe_len)
            .and_then(|r| r.checked_add(stripe_offset))
            .and_then(|within| stripe.offset.checked_add(within))
            .ok_or_else(overflow)?;

        let to_chunk_end = self.logical_end() - logical;
        let len = if profile.is_striped()
            || matches!(profile, ChunkProfile::Raid5 | ChunkProfile::Raid6)
        {
            (self.stripe_len - stripe_offset).min(to_chunk_end)
        } else {
            to_chunk_end
        };

        Ok(Mapping {
            devid: stripe.devid,
            physical,
            len,
        })
    }
}

/// Where the elements of one RAID5/6 full stripe are, from
/// [`Chunk::parity_read`]. Each is `(devid, physical)` at the same offset
/// into its element as the address asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityRead {
    /// The data element the address is in.
    pub want: usize,
    /// Every data element, by data index.
    pub data: Vec<(u64, u64)>,
    /// P.
    pub p: (u64, u64),
    /// Q, for RAID6.
    pub q: Option<(u64, u64)>,
    /// Bytes from the address to the end of its element or of the chunk.
    pub len: u64,
}

/// The logical-to-physical address map: an ordered, non-overlapping set
/// of chunks.
///
/// Bootstrapped from the superblock's `sys_chunk_array` and then extended
/// with the chunk items found in the chunk tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMap {
    chunks: Vec<Chunk>,
}

/// The chunk an item of the chunk tree or the `sys_chunk_array` describes,
/// or `None` for an item that is not a chunk.
///
/// ONE PLACE FOR BOTH BUILDERS (#85). The superblock array checked the key
/// type, the objectid and the geometry; the chunk-tree walk checked none
/// of them and dropped any item that failed to parse. So a `DEV_ITEM` --
/// which the chunk tree also holds, one per device -- was parsed as a
/// chunk at logical address `devid`, kept out of the map only by its
/// field values; a malformed `CHUNK_ITEM` was discarded and its range
/// surfaced much later as an unmapped read somewhere unrelated; and the
/// geometry check ran on the handful of bootstrap chunks and not on the
/// tree's.
///
/// # Errors
///
/// [`Error::BadChunkItem`] for a `CHUNK_ITEM` under the wrong objectid,
/// one that does not parse, or one whose geometry is not sector-aligned.
pub fn chunk_from_item(key: &DiskKey, data: &[u8], sectorsize: u32) -> Result<Option<Chunk>> {
    if key.key_type != key_type::CHUNK_ITEM {
        return Ok(None);
    }
    if key.objectid != objectid::FIRST_CHUNK_TREE {
        return Err(Error::BadChunkItem(format!(
            "chunk item at logical {:#x} has objectid {}, expected {}",
            key.offset,
            key.objectid,
            objectid::FIRST_CHUNK_TREE
        )));
    }
    // Both refusals name the item, so a mount that fails here points at
    // the chunk rather than at a reason with no address.
    let named = |e| match e {
        Error::BadChunkItem(why) => {
            Error::BadChunkItem(format!("chunk item at logical {:#x}: {why}", key.offset))
        }
        other => other,
    };
    let chunk = Chunk::parse(key.offset, data).map_err(named)?;
    chunk.validate_geometry(sectorsize).map_err(named)?;
    Ok(Some(chunk))
}

impl ChunkMap {
    /// An empty map.
    pub fn new() -> Self {
        ChunkMap::default()
    }

    /// Build the bootstrap map from a superblock's system chunk array.
    ///
    /// This is the one step that breaks the circular dependency between
    /// "read the chunk tree" and "translate a logical address": the array
    /// is a flat list, addressed physically, that covers enough of the
    /// logical space to reach the chunk tree root.
    ///
    /// # Errors
    ///
    /// [`Error::BadChunkItem`] if the array is truncated mid-entry, holds
    /// a key that is not a chunk item, or contains a chunk whose geometry
    /// does not hold up.
    pub fn from_sys_chunk_array(array: &[u8], sectorsize: u32) -> Result<Self> {
        let mut map = ChunkMap::new();
        let mut pos = 0usize;
        while pos < array.len() {
            let key = DiskKey::parse(&array[pos..]).map_err(|_| {
                Error::BadChunkItem(format!(
                    "sys_chunk_array truncated: {} bytes left, need {DISK_KEY_SIZE} for a key",
                    array.len() - pos
                ))
            })?;
            if key.key_type != key_type::CHUNK_ITEM {
                return Err(Error::BadChunkItem(format!(
                    "sys_chunk_array entry at byte {pos} has key type {}, expected {}",
                    key.key_type,
                    key_type::CHUNK_ITEM
                )));
            }
            if key.objectid != objectid::FIRST_CHUNK_TREE {
                return Err(Error::BadChunkItem(format!(
                    "sys_chunk_array entry at byte {pos} has objectid {}, expected {}",
                    key.objectid,
                    objectid::FIRST_CHUNK_TREE
                )));
            }
            pos += DISK_KEY_SIZE;
            let chunk = chunk_from_item(&key, &array[pos..], sectorsize)?
                .expect("the key type was checked above");
            pos += chunk.encoded_len();
            map.insert(chunk)?;
        }
        Ok(map)
    }

    /// Build the bootstrap map straight from a parsed superblock.
    pub fn bootstrap(sb: &Superblock) -> Result<Self> {
        Self::from_sys_chunk_array(&sb.sys_chunk_array, sb.sectorsize)
    }

    /// Add a chunk, keeping the map ordered by logical address.
    ///
    /// # Errors
    ///
    /// [`Error::BadChunkItem`] if the new chunk's logical range overlaps
    /// one already present. Overlapping chunks would make translation
    /// ambiguous, and the ambiguity would show up much later as
    /// unexplained data corruption.
    pub fn insert(&mut self, chunk: Chunk) -> Result<()> {
        let at = self.chunks.partition_point(|c| c.logical < chunk.logical);
        if let Some(prev) = at.checked_sub(1).and_then(|i| self.chunks.get(i)) {
            if prev.logical_end() > chunk.logical {
                return Err(Error::BadChunkItem(format!(
                    "chunk at {:#x} overlaps the chunk at {:#x}",
                    chunk.logical, prev.logical
                )));
            }
        }
        if let Some(next) = self.chunks.get(at) {
            if chunk.logical_end() > next.logical {
                return Err(Error::BadChunkItem(format!(
                    "chunk at {:#x} overlaps the chunk at {:#x}",
                    chunk.logical, next.logical
                )));
            }
        }
        self.chunks.insert(at, chunk);
        Ok(())
    }

    /// Every chunk, ordered by logical address.
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// How many chunks the map holds.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether the map is empty. An empty map cannot translate anything.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The chunk covering `logical`, if any.
    pub fn chunk_for(&self, logical: u64) -> Option<&Chunk> {
        let at = self.chunks.partition_point(|c| c.logical <= logical);
        let candidate = self.chunks.get(at.checked_sub(1)?)?;
        if logical < candidate.logical_end() {
            Some(candidate)
        } else {
            None
        }
    }

    /// Whether any chunk covers `logical`.
    pub fn covers(&self, logical: u64) -> bool {
        self.chunk_for(logical).is_some()
    }

    /// How many copies of `logical` exist.
    ///
    /// One for `Single`, two for `Dup` and `Raid1`, and so on. A WRITE
    /// has to reach every one of them: leaving a mirror holding the
    /// previous contents is not a lesser form of redundancy, it is a
    /// filesystem whose two copies disagree, and which copy a later read
    /// returns is not something the caller chooses.
    pub fn mirrors_at(&self, logical: u64) -> Result<usize> {
        Ok(self
            .chunk_for(logical)
            .ok_or(Error::UnmappedLogical(logical))?
            .num_mirrors())
    }

    /// How many ways there are to read `logical`; see
    /// [`Chunk::read_attempts`].
    pub fn read_attempts_at(&self, logical: u64) -> Result<usize> {
        Ok(self
            .chunk_for(logical)
            .ok_or(Error::UnmappedLogical(logical))?
            .read_attempts())
    }

    /// Translate `logical` using the first copy.
    pub fn map(&self, logical: u64) -> Result<Mapping> {
        self.map_mirror(logical, 0)
    }

    /// Translate `logical` using copy `mirror`.
    pub fn map_mirror(&self, logical: u64, mirror: usize) -> Result<Mapping> {
        self.chunk_for(logical)
            .ok_or(Error::UnmappedLogical(logical))?
            .map_mirror(logical, mirror)
    }
}
