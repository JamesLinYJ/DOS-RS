//! Btrfs superblock parsing and validation.
//!
//! # Byte order
//!
//! **Btrfs stores every multi-byte on-disk field little-endian**, on
//! every host, including the checksum. That uniformity is worth stating
//! explicitly because the sister XFS driver in this family is the exact
//! opposite — big-endian everywhere *except* its CRC, which is stored
//! little-endian. A reader that carries the XFS habit across will get
//! Btrfs wrong in a way no round-trip test can see: a writer and reader
//! that share a byte-order mistake agree with each other perfectly while
//! disagreeing with every real filesystem.
//!
//! # Layout
//!
//! The superblock is a fixed [`SUPER_INFO_SIZE`]-byte (4 KiB) structure
//! written at up to three fixed *physical* offsets — see
//! [`SUPER_OFFSETS`]. Copy 0 at 64 KiB is the primary; a copy is only
//! written when the device is large enough to hold it. Every copy is
//! self-describing: `bytenr` records the offset the copy belongs at, so a
//! misdirected read is detectable independently of the checksum.
//!
//! # Checksum
//!
//! The first [`CSUM_SIZE`] (32) bytes hold the checksum. It covers bytes
//! `0x20 .. 0x1000` — that is, the whole 4 KiB superblock *minus* the
//! checksum field itself, not merely the fields this driver happens to
//! parse and not merely up to the end of some sub-structure. Getting that
//! span wrong is invisible to a self-built fixture and fatal on real
//! media.
//!
//! Only the first `csum_type.digest_len()` bytes of the 32-byte field are
//! meaningful; the writer zero-fills the rest and the kernel compares
//! only the meaningful prefix. This driver does the same.
//!
//! # Confidence
//!
//! Every offset in [`offsets`] was taken from the published on-disk
//! format tables and cross-checked against the field order of
//! `struct btrfs_super_block`. Anything that could not be corroborated by
//! two independent sources is called out in a comment at its use site.

use alloc::{vec::Vec, string::String, string::ToString};
use crate::error::{Error, Result};

/// The eight magic bytes at [`offsets::MAGIC`], in on-disk order.
///
/// Spelled as bytes rather than as an integer on purpose. Btrfs's magic
/// is an ASCII string, and writing it as a `u64` invites exactly the
/// transposition bug that is impossible to spot in a hand-built fixture:
/// the fixture writer and the parser share the mistake and agree.
/// [`BTRFS_MAGIC_LE64`] records the little-endian integer form for
/// cross-reference, and a unit test asserts the two agree.
pub const BTRFS_MAGIC: [u8; 8] = *b"_BHRfS_M";

/// [`BTRFS_MAGIC`] as the little-endian `u64` the format documentation
/// also quotes it as: `0x4D5F_5366_5248_425F`.
pub const BTRFS_MAGIC_LE64: u64 = 0x4D5F_5366_5248_425F;

/// Size of the on-disk superblock structure in bytes (`BTRFS_SUPER_INFO_SIZE`).
pub const SUPER_INFO_SIZE: usize = 4096;

/// Physical byte offset of the primary superblock (64 KiB).
pub const SUPER_INFO_OFFSET: u64 = 0x1_0000;

/// Every physical offset a superblock copy may live at.
///
/// Copy 0 is the primary at 64 KiB; copies 1 and 2 are at 64 MiB and
/// 256 GiB. The kernel derives these as `16 KiB << (12 * mirror)` for
/// `mirror > 0` and caps the count at three, so there is deliberately no
/// 1 PiB copy even though the shift sequence would produce one.
pub const SUPER_OFFSETS: [u64; 3] = [SUPER_INFO_OFFSET, 0x400_0000, 0x40_0000_0000];

/// Size of the checksum field (`BTRFS_CSUM_SIZE`). Wide enough for the
/// largest supported digest; narrower digests are zero-padded.
pub const CSUM_SIZE: usize = 32;

/// Size of an FS/device UUID field (`BTRFS_FSID_SIZE` / `BTRFS_UUID_SIZE`).
pub const UUID_SIZE: usize = 16;

/// Size of the label field (`BTRFS_LABEL_SIZE`), NUL-padded.
pub const LABEL_SIZE: usize = 256;

/// Size of the embedded system chunk array (`BTRFS_SYSTEM_CHUNK_ARRAY_SIZE`).
pub const SYS_CHUNK_ARRAY_SIZE: usize = 2048;

/// Size of the embedded `struct btrfs_dev_item`.
pub const DEV_ITEM_SIZE: usize = 0x62;

/// Maximum B-tree height (`BTRFS_MAX_LEVEL`). A `*_level` field at or
/// above this is nonsense.
pub const MAX_LEVEL: u8 = 8;

/// Largest metadata block size Btrfs permits (`BTRFS_MAX_METADATA_BLOCKSIZE`).
pub const MAX_METADATA_BLOCKSIZE: u32 = 65536;

/// Smallest sector size Btrfs permits.
pub const MIN_SECTORSIZE: u32 = 4096;

/// Byte offsets of every superblock field, relative to the start of the
/// structure.
///
/// Kept as named constants rather than inlined numbers so that a reader
/// can diff this block against the format documentation directly, and so
/// that a cross-validation harness can assert against the same names the
/// parser uses.
pub mod offsets {
    /// Checksum of bytes `0x20 .. 0x1000`.
    pub const CSUM: usize = 0x00;
    /// Filesystem UUID.
    pub const FSID: usize = 0x20;
    /// Physical byte offset this superblock copy belongs at.
    pub const BYTENR: usize = 0x30;
    /// Superblock flags — see [`super::super_flags`].
    pub const FLAGS: usize = 0x38;
    /// `_BHRfS_M`.
    pub const MAGIC: usize = 0x40;
    /// Transaction id that wrote this superblock.
    pub const GENERATION: usize = 0x48;
    /// Logical address of the root tree's root node.
    pub const ROOT: usize = 0x50;
    /// Logical address of the chunk tree's root node.
    pub const CHUNK_ROOT: usize = 0x58;
    /// Logical address of the log tree's root node, 0 when clean.
    pub const LOG_ROOT: usize = 0x60;
    /// Legacy `log_root_transid`; unused by modern kernels.
    pub const LOG_ROOT_TRANSID: usize = 0x68;
    /// Total size of the filesystem across all devices.
    pub const TOTAL_BYTES: usize = 0x70;
    /// Bytes currently allocated.
    pub const BYTES_USED: usize = 0x78;
    /// Objectid of the root directory (conventionally 6).
    pub const ROOT_DIR_OBJECTID: usize = 0x80;
    /// Number of devices in the filesystem.
    pub const NUM_DEVICES: usize = 0x88;
    /// Sector size in bytes.
    pub const SECTORSIZE: usize = 0x90;
    /// Metadata node size in bytes.
    pub const NODESIZE: usize = 0x94;
    /// Legacy `leafsize`; modern kernels call this `__unused_leafsize`
    /// and mkfs writes `nodesize` into it.
    pub const LEAFSIZE: usize = 0x98;
    /// Stripe size in bytes; modern kernels require it to equal
    /// `sectorsize`.
    pub const STRIPESIZE: usize = 0x9c;
    /// Valid byte count within [`SYS_CHUNK_ARRAY`].
    pub const SYS_CHUNK_ARRAY_SIZE: usize = 0xa0;
    /// Transaction id of the chunk tree root.
    pub const CHUNK_ROOT_GENERATION: usize = 0xa4;
    /// Compatible feature mask.
    pub const COMPAT_FLAGS: usize = 0xac;
    /// Read-only-compatible feature mask.
    pub const COMPAT_RO_FLAGS: usize = 0xb4;
    /// Incompatible feature mask.
    pub const INCOMPAT_FLAGS: usize = 0xbc;
    /// Metadata checksum algorithm selector.
    pub const CSUM_TYPE: usize = 0xc4;
    /// Height of the root tree.
    pub const ROOT_LEVEL: usize = 0xc6;
    /// Height of the chunk tree.
    pub const CHUNK_ROOT_LEVEL: usize = 0xc7;
    /// Height of the log tree.
    pub const LOG_ROOT_LEVEL: usize = 0xc8;
    /// Embedded `struct btrfs_dev_item` describing *this* device.
    pub const DEV_ITEM: usize = 0xc9;
    /// NUL-padded volume label.
    pub const LABEL: usize = 0x12b;
    /// Free-space-cache generation.
    pub const CACHE_GENERATION: usize = 0x22b;
    /// UUID tree generation.
    pub const UUID_TREE_GENERATION: usize = 0x233;
    /// UUID stamped into tree node headers when the `METADATA_UUID`
    /// incompatible feature is set.
    ///
    /// This one field sits in what the older published format tables
    /// still describe as a 240-byte reserved run. Its position is derived
    /// from the field order of `struct btrfs_super_block`, and the
    /// arithmetic closes exactly: `0x23b + 16` (metadata_uuid) `+ 8`
    /// (nr_global_roots) `+ 216` (the remaining reserved bytes) lands on
    /// [`SYS_CHUNK_ARRAY`] at `0x32b`. Worth confirming against a real
    /// `mkfs.btrfs -U`/`btrfstune -m` image all the same.
    pub const METADATA_UUID: usize = 0x23b;
    /// Number of global roots (extent-tree-v2 era). Zero on every volume
    /// this driver will accept.
    pub const NR_GLOBAL_ROOTS: usize = 0x24b;
    /// Bootstrap chunk mappings — `(key, chunk item)` pairs.
    pub const SYS_CHUNK_ARRAY: usize = 0x32b;
    /// Four backup root records.
    pub const SUPER_ROOTS: usize = 0xb2b;
}

/// Byte offsets within the embedded `struct btrfs_dev_item`, relative to
/// [`offsets::DEV_ITEM`].
pub mod dev_item_offsets {
    /// Device id.
    pub const DEVID: usize = 0x00;
    /// Device capacity in bytes.
    pub const TOTAL_BYTES: usize = 0x08;
    /// Bytes allocated on this device.
    pub const BYTES_USED: usize = 0x10;
    /// Optimal I/O alignment.
    pub const IO_ALIGN: usize = 0x18;
    /// Optimal I/O width.
    pub const IO_WIDTH: usize = 0x1c;
    /// Minimal I/O size.
    pub const SECTOR_SIZE: usize = 0x20;
    /// Device type flags.
    pub const TYPE: usize = 0x24;
    /// Generation.
    pub const GENERATION: usize = 0x2c;
    /// Start offset — bytes at the front of the device left unused.
    pub const START_OFFSET: usize = 0x34;
    /// Device group.
    pub const DEV_GROUP: usize = 0x3c;
    /// Seek speed hint.
    pub const SEEK_SPEED: usize = 0x40;
    /// Bandwidth hint.
    pub const BANDWIDTH: usize = 0x41;
    /// Device UUID.
    pub const UUID: usize = 0x42;
    /// UUID of the filesystem this device belongs to.
    pub const FSID: usize = 0x52;
}

/// Superblock `flags` bits.
pub mod super_flags {
    /// The filesystem was marked as having hit an error.
    pub const ERROR: u64 = 1 << 2;
    /// This device is a read-only seed for another filesystem.
    pub const SEEDING: u64 = 1 << 32;
    /// Image produced by a metadata-only dump tool; data extents are
    /// absent, so the volume is not mountable.
    pub const METADUMP: u64 = 1 << 33;
    /// Second-generation metadata-only dump.
    pub const METADUMP_V2: u64 = 1 << 34;
    /// A `fsid` change is in flight.
    pub const CHANGING_FSID: u64 = 1 << 35;
    /// A `fsid` change is in flight (v2 scheme).
    pub const CHANGING_FSID_V2: u64 = 1 << 36;
}

/// `incompat_flags` bits. A volume setting a bit this driver does not
/// understand cannot be read at all, let alone written.
pub mod incompat {
    /// Extent back-references use the mixed format. Universal since 2.6.31.
    pub const MIXED_BACKREF: u64 = 1 << 0;
    /// A non-default subvolume is set as the mount default.
    pub const DEFAULT_SUBVOL: u64 = 1 << 1;
    /// Data and metadata share block groups (small-volume layout).
    pub const MIXED_GROUPS: u64 = 1 << 2;
    /// Some extents may be LZO-compressed.
    pub const COMPRESS_LZO: u64 = 1 << 3;
    /// Some extents may be zstd-compressed.
    pub const COMPRESS_ZSTD: u64 = 1 << 4;
    /// Metadata nodes may be larger than one page. Universal since 3.4.
    pub const BIG_METADATA: u64 = 1 << 5;
    /// Extended inode references (long/many hard links).
    pub const EXTENDED_IREF: u64 = 1 << 6;
    /// The volume contains RAID5 or RAID6 chunks.
    pub const RAID56: u64 = 1 << 7;
    /// Metadata back-references use the compact "skinny" key form.
    pub const SKINNY_METADATA: u64 = 1 << 8;
    /// File holes are implicit — no explicit hole extents are recorded.
    pub const NO_HOLES: u64 = 1 << 9;
    /// Tree nodes are stamped with `metadata_uuid` instead of `fsid`.
    pub const METADATA_UUID: u64 = 1 << 10;
    /// RAID1C3 and RAID1C4 (three- and four-way mirror) profiles.
    pub const RAID1C34: u64 = 1 << 11;
    /// Zoned-device layout — sequential-write zones, no in-place update.
    pub const ZONED: u64 = 1 << 12;
    /// Second-generation extent tree (global roots, per-block-group trees).
    pub const EXTENT_TREE_V2: u64 = 1 << 13;
    /// Separate tree recording RAID stripe placement.
    pub const RAID_STRIPE_TREE: u64 = 1 << 14;
    /// Simple (non-hierarchical) quota accounting.
    ///
    /// Bit 15 is unassigned; simple quotas take bit 16. Both this and
    /// [`REMAP_TREE`] are recent enough that they are worth re-checking
    /// against the kernel a reader is expected to interoperate with.
    pub const SIMPLE_QUOTA: u64 = 1 << 16;
    /// Device remap tree.
    pub const REMAP_TREE: u64 = 1 << 17;

    /// Every incompatible feature this driver can read.
    ///
    /// The exclusions are deliberate, not accidental:
    ///
    /// - [`EXTENT_TREE_V2`], [`RAID_STRIPE_TREE`],
    ///   [`REMAP_TREE`] — each restructures something the read path
    ///   depends on. [`super::refuse_unreadable_incompat`] refuses them.
    ///
    /// [`ZONED`] is readable on a regular device with fixed-offset
    /// superblocks. `read_superblock` refuses a real zoned device's
    /// superblock log, and `mount_rw` refuses all zoned writes (#454).
    ///
    /// [`SIMPLE_QUOTA`] IS accepted (#270): it adds an owner reference to
    /// each data extent item and keeps usage in the quota tree, neither of
    /// which moves a file's bytes. `mount_rw` refuses it, because a write
    /// would allocate extents without either.
    ///
    /// [`COMPRESS_LZO`] and [`COMPRESS_ZSTD`] ARE accepted, which was
    /// not always so. The concern that kept them out was a reader
    /// handing back compressed bytes as if they were file contents — a
    /// failure a caller cannot detect. That risk is now handled where it
    /// actually arises: the extent decoder refuses a compressed extent
    /// by name, so a compressed file errors and an uncompressed one
    /// reads normally.
    ///
    /// Refusing the whole mount instead was the worse trade. The bit
    /// means *some* extent may be compressed, not that the layout is
    /// unreadable, so gating on it made an entire filesystem
    /// unmountable because one file happened to compress well.
    pub const SUPPORTED: u64 = MIXED_BACKREF
        | DEFAULT_SUBVOL
        | MIXED_GROUPS
        | COMPRESS_LZO
        | COMPRESS_ZSTD
        | BIG_METADATA
        | EXTENDED_IREF
        | SKINNY_METADATA
        | NO_HOLES
        | METADATA_UUID
        | RAID1C34
        // Read: the data elements are placed round the rotating parity
        // and a damaged one is rebuilt from it (#268), against pools the
        // kernel wrote. Never written: `mount_rw` takes one device, and a
        // parity chunk spans several.
        | RAID56
        | ZONED
        | SIMPLE_QUOTA;
}

/// Refuse a volume carrying an incompatible feature this driver cannot
/// read, naming each feature it knows (#447).
///
/// A bit outside [`incompat::SUPPORTED`] means the kernel that wrote the
/// volume laid something out in a way this driver does not understand, so
/// reading it at all would be guessing. The refusal used to give only the
/// mask, which says nothing a user can act on; a known feature is named,
/// and a bit no feature is known by is still given by its value.
pub fn refuse_unreadable_incompat(incompat_flags: u64) -> Result<()> {
    let mut unreadable = incompat_flags & !incompat::SUPPORTED;
    if unreadable == 0 {
        return Ok(());
    }
    let mut named = Vec::new();
    for (bit, name) in [
        (
            incompat::EXTENT_TREE_V2,
            "extent-tree-v2 (the second-generation extent tree)",
        ),
        (
            incompat::RAID_STRIPE_TREE,
            "raid-stripe-tree (the tree that records data stripe placement)",
        ),
        (
            incompat::REMAP_TREE,
            "remap-tree (the tree that remaps block addresses)",
        ),
    ] {
        if unreadable & bit != 0 {
            named.push(name.to_string());
            unreadable &= !bit;
        }
    }
    if unreadable != 0 {
        named.push(format!("incompatible feature bits {unreadable:#018x}"));
    }
    Err(Error::UnsupportedFeature(format!(
        "this volume uses incompatible features this driver cannot read ({}), so it cannot \
         be mounted",
        named.join(", ")
    )))
}

/// `compat_ro_flags` bits: features an implementation may ignore while
/// READING and must not ignore while WRITING.
///
/// A read-only mount accepts any of them. [`Filesystem::mount_rw`] refuses
/// a volume with a bit outside [`compat_ro::SUPPORTED`] -- this driver
/// writes, and a writer that ignores a structure the bit announces leaves
/// it describing a filesystem that no longer exists (#72). See
/// [`refuse_unmaintained_compat_ro`].
///
/// [`Filesystem::mount_rw`]: crate::fs::Filesystem::mount_rw
pub mod compat_ro {
    /// A free-space tree (space_cache v2) is present.
    pub const FREE_SPACE_TREE: u64 = 1 << 0;
    /// The free-space tree is up to date.
    pub const FREE_SPACE_TREE_VALID: u64 = 1 << 1;
    /// fs-verity metadata may be present.
    pub const VERITY: u64 = 1 << 2;
    /// Block group items live in their own tree.
    pub const BLOCK_GROUP_TREE: u64 = 1 << 3;

    /// Every `compat_ro` feature this driver MAINTAINS when it writes.
    ///
    /// The free-space tree is updated by every transaction beside the
    /// extent tree it complements, and its validity bit is cleared when a
    /// commit could not, which is the state the format defines for that.
    /// Block group items are maintained in tree 11 when `BLOCK_GROUP_TREE`
    /// is set, including when chunks are created or removed.
    /// Verity metadata stays unchanged because sealed inodes refuse data
    /// mutations and removal.
    pub const SUPPORTED: u64 = FREE_SPACE_TREE | FREE_SPACE_TREE_VALID | BLOCK_GROUP_TREE | VERITY;
}

/// Refuse to WRITE a volume carrying a `compat_ro` feature this driver
/// does not maintain (#72).
///
/// Reading such a volume is sound, which is what the bit's class says, so
/// this is checked at `mount_rw` and not at parse.
pub fn refuse_unmaintained_compat_ro(compat_ro_flags: u64) -> Result<()> {
    let unmaintained = compat_ro_flags & !compat_ro::SUPPORTED;
    if unmaintained == 0 {
        return Ok(());
    }
    Err(Error::UnsupportedFeature(format!(
        "this volume uses read-only-compatible features this driver does not maintain \
         (unknown bits {unmaintained:#018x}), so it can be read but not written"
    )))
}


/// The hash algorithm protecting this volume's metadata, from
/// `csum_type` at [`offsets::CSUM_TYPE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumType {
    /// CRC32C (Castagnoli), 4-byte digest. The `mkfs.btrfs` default.
    Crc32c,
    /// XXH64 with seed 0, 8-byte digest.
    XxHash64,
    /// SHA-256, 32-byte digest.
    Sha256,
    /// BLAKE2b truncated to 256 bits, 32-byte digest.
    Blake2b256,
}

impl ChecksumType {
    /// Decode the on-disk `csum_type` selector.
    pub fn from_raw(raw: u16) -> Result<Self> {
        match raw {
            0 => Ok(ChecksumType::Crc32c),
            1 => Ok(ChecksumType::XxHash64),
            2 => Ok(ChecksumType::Sha256),
            3 => Ok(ChecksumType::Blake2b256),
            other => Err(Error::UnsupportedChecksum(other)),
        }
    }

    /// The on-disk selector value.
    pub fn to_raw(self) -> u16 {
        match self {
            ChecksumType::Crc32c => 0,
            ChecksumType::XxHash64 => 1,
            ChecksumType::Sha256 => 2,
            ChecksumType::Blake2b256 => 3,
        }
    }

    /// How many of the 32 checksum bytes this algorithm actually fills.
    /// The remainder is zero-padding and is not compared.
    pub fn digest_len(self) -> usize {
        match self {
            ChecksumType::Crc32c => 4,
            ChecksumType::XxHash64 => 8,
            ChecksumType::Sha256 | ChecksumType::Blake2b256 => 32,
        }
    }

    /// Compute the digest of `data` into a zero-padded 32-byte field, in
    /// the exact on-disk representation.
    ///
    /// The CRC32C and XXH64 digests are stored **little-endian**, like
    /// every other integer in the format. That is worth spelling out
    /// because the sister XFS driver stores its CRC little-endian while
    /// storing everything else big-endian, and the instinct to treat a
    /// checksum as "the odd one out" does not transfer here.
    pub fn digest(self, data: &[u8]) -> [u8; CSUM_SIZE] {
        let mut out = [0u8; CSUM_SIZE];
        match self {
            ChecksumType::Crc32c => {
                out[..4].copy_from_slice(&crate::crc32c::crc32c(data).to_le_bytes());
            }
            ChecksumType::XxHash64 => {
                let h = twox_hash::XxHash64::oneshot(0, data);
                out[..8].copy_from_slice(&h.to_le_bytes());
            }
            ChecksumType::Sha256 => {
                use sha2::Digest;
                let h = sha2::Sha256::digest(data);
                out.copy_from_slice(&h);
            }
            ChecksumType::Blake2b256 => {
                let h = blake2b_simd::Params::new().hash_length(32).hash(data);
                out.copy_from_slice(h.as_bytes());
            }
        }
        out
    }

    /// Whether `stored` matches the digest of `data`. Only the first
    /// [`Self::digest_len`] bytes participate, matching what the kernel
    /// compares.
    pub fn verify(self, data: &[u8], stored: &[u8]) -> bool {
        let n = self.digest_len();
        if stored.len() < n {
            return false;
        }
        self.digest(data)[..n] == stored[..n]
    }
}

/// The `struct btrfs_dev_item` embedded in the superblock, describing the
/// device the superblock was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevItem {
    /// Device id — the value chunk stripes reference.
    pub devid: u64,
    /// Device capacity in bytes.
    pub total_bytes: u64,
    /// Bytes of this device claimed by chunks.
    pub bytes_used: u64,
    /// Optimal I/O alignment.
    pub io_align: u32,
    /// Optimal I/O width.
    pub io_width: u32,
    /// Minimal I/O size.
    pub sector_size: u32,
    /// Device type flags.
    pub dev_type: u64,
    /// Generation.
    pub generation: u64,
    /// Bytes at the front of the device that are not used by the
    /// filesystem.
    pub start_offset: u64,
    /// Device group.
    pub dev_group: u32,
    /// Seek speed hint.
    pub seek_speed: u8,
    /// Bandwidth hint.
    pub bandwidth: u8,
    /// This device's UUID.
    pub uuid: [u8; UUID_SIZE],
    /// The owning filesystem's UUID. Holds `metadata_uuid` rather than
    /// `fsid` when the `METADATA_UUID` incompatible feature is set.
    pub fsid: [u8; UUID_SIZE],
}

/// A superblock that is out of range or internally inconsistent.
/// [`Error::BadSuperblock`] covers every malformed metadata structure,
/// so the message names the superblock itself (#473).
fn malformed(m: String) -> Error {
    Error::BadSuperblock(format!("superblock: {m}"))
}

impl DevItem {
    /// Parse a device item from `buf`, which must be at least
    /// [`DEV_ITEM_SIZE`] bytes.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        use dev_item_offsets as o;
        if buf.len() < DEV_ITEM_SIZE {
            return Err(Error::BadSuperblock(format!(
                "dev_item needs {DEV_ITEM_SIZE} bytes, got {}",
                buf.len()
            )));
        }
        Ok(DevItem {
            devid: le64(buf, o::DEVID),
            total_bytes: le64(buf, o::TOTAL_BYTES),
            bytes_used: le64(buf, o::BYTES_USED),
            io_align: le32(buf, o::IO_ALIGN),
            io_width: le32(buf, o::IO_WIDTH),
            sector_size: le32(buf, o::SECTOR_SIZE),
            dev_type: le64(buf, o::TYPE),
            generation: le64(buf, o::GENERATION),
            start_offset: le64(buf, o::START_OFFSET),
            dev_group: le32(buf, o::DEV_GROUP),
            seek_speed: buf[o::SEEK_SPEED],
            bandwidth: buf[o::BANDWIDTH],
            uuid: uuid_at(buf, o::UUID),
            fsid: uuid_at(buf, o::FSID),
        })
    }
}

/// A parsed Btrfs superblock.
///
/// Field names follow the on-disk names so a reader can match this
/// against the format documentation without a translation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superblock {
    /// The raw 32-byte checksum field, as stored.
    pub csum: [u8; CSUM_SIZE],
    /// Filesystem UUID.
    pub fsid: [u8; UUID_SIZE],
    /// Physical byte offset this copy belongs at.
    pub bytenr: u64,
    /// Superblock flags — see [`super_flags`].
    pub flags: u64,
    /// Transaction id that wrote this superblock.
    pub generation: u64,
    /// Logical address of the root tree's root node.
    pub root: u64,
    /// Logical address of the chunk tree's root node.
    pub chunk_root: u64,
    /// Logical address of the log tree's root node; 0 when clean.
    pub log_root: u64,
    /// Legacy `log_root_transid`, retained for completeness.
    pub log_root_transid: u64,
    /// Total size of the filesystem across all devices.
    pub total_bytes: u64,
    /// Bytes currently allocated.
    pub bytes_used: u64,
    /// Objectid of the root directory (conventionally 6).
    pub root_dir_objectid: u64,
    /// Number of devices in the filesystem.
    pub num_devices: u64,
    /// Sector size in bytes.
    pub sectorsize: u32,
    /// Metadata node size in bytes.
    pub nodesize: u32,
    /// Legacy `leafsize`. Modern kernels ignore it entirely; it is parsed
    /// but deliberately not validated, because old images are free to
    /// disagree with `nodesize` here and rejecting them would be wrong.
    pub leafsize: u32,
    /// Stripe size in bytes.
    pub stripesize: u32,
    /// Valid byte count within the system chunk array.
    pub sys_chunk_array_size: u32,
    /// Transaction id of the chunk tree root.
    pub chunk_root_generation: u64,
    /// Compatible feature mask.
    pub compat_flags: u64,
    /// Read-only-compatible feature mask.
    pub compat_ro_flags: u64,
    /// Incompatible feature mask.
    pub incompat_flags: u64,
    /// Metadata checksum algorithm.
    pub csum_type: ChecksumType,
    /// Height of the root tree.
    pub root_level: u8,
    /// Height of the chunk tree.
    pub chunk_root_level: u8,
    /// Height of the log tree.
    pub log_root_level: u8,
    /// Device item for the device this superblock was read from.
    pub dev_item: DevItem,
    /// Volume label, trailing NULs trimmed.
    pub label: String,
    /// Free-space-cache generation.
    pub cache_generation: u64,
    /// UUID tree generation.
    pub uuid_tree_generation: u64,
    /// UUID stamped into tree node headers when `METADATA_UUID` is set.
    /// Use [`Self::node_uuid`] rather than reading this directly.
    pub metadata_uuid: [u8; UUID_SIZE],
    /// Number of global roots (extent-tree-v2 era).
    pub nr_global_roots: u64,
    /// The valid prefix of the embedded system chunk array, copied out.
    /// Feed this to [`crate::chunk::ChunkMap::from_sys_chunk_array`].
    pub sys_chunk_array: Vec<u8>,
}

/// Reading fixed-width little-endian fields out of a byte slice.
///
/// Btrfs is little-endian throughout. There is deliberately no
/// big-endian half: a big-endian read in this crate would be a bug, and
/// a helper for it would make the bug spellable.
///
/// These live here rather than in an `endian` module of their own
/// because the superblock is the first thing anything parses, and every
/// other module already imports from it. `block_group` and `fs` each
/// used to declare their own copies — three definitions of `le64` in
/// one crate, byte-identical.
///
/// # Panics
///
/// If the slice is too short. Every caller has already length-checked
/// its buffer against the structure it is parsing; that check belongs
/// where the structure's size is known, not repeated per field. A panic
/// here means a caller skipped it, which is a bug in this crate rather
/// than bad input.
///
/// Read a little-endian `u16` at `off`.
#[inline]
pub(crate) fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().expect("2 bytes"))
}

/// Read a little-endian `u32` at `off`.
#[inline]
pub(crate) fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().expect("4 bytes"))
}

/// Read a little-endian `u64` at `off`.
#[inline]
pub(crate) fn le64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().expect("8 bytes"))
}

/// Copy a 16-byte UUID out at `off`.
#[inline]
pub(crate) fn uuid_at(b: &[u8], off: usize) -> [u8; UUID_SIZE] {
    b[off..off + UUID_SIZE].try_into().expect("16 bytes")
}

impl Superblock {
    /// Parse and validate a superblock from the first [`SUPER_INFO_SIZE`]
    /// bytes of `buf`.
    ///
    /// The full 4 KiB must be present even though the parsed fields stop
    /// well short of it: the checksum covers the whole structure.
    ///
    /// # Errors
    ///
    /// [`Error::NotBtrfs`] if the magic does not match,
    /// [`Error::UnsupportedChecksum`] if `csum_type` names an algorithm
    /// this driver has no implementation for,
    /// [`Error::ChecksumMismatch`] if the stored digest disagrees,
    /// [`Error::UnsupportedFeature`] if an incompatible feature bit this
    /// driver does not implement is set, and [`Error::BadSuperblock`] if
    /// a geometry field is out of range or internally inconsistent.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < SUPER_INFO_SIZE {
            return Err(malformed(format!(
                "need {SUPER_INFO_SIZE} bytes, got {}",
                buf.len()
            )));
        }

        let magic: [u8; 8] = buf[offsets::MAGIC..offsets::MAGIC + 8]
            .try_into()
            .expect("8 bytes");
        if magic != BTRFS_MAGIC {
            return Err(Error::NotBtrfs { magic });
        }

        // csum_type has to be trusted just far enough to pick an
        // algorithm, before the checksum that protects it can be
        // verified. That is unavoidable, so it is range-checked first and
        // an unknown value is rejected rather than used.
        let csum_type = ChecksumType::from_raw(le16(buf, offsets::CSUM_TYPE))?;

        // The checksum covers 0x20..0x1000 — the whole superblock minus
        // the checksum field. Not "the fields we parse", not "up to
        // sys_chunk_array". A shorter span passes on fixtures this crate
        // builds itself and fails on every filesystem the kernel made.
        let csum: [u8; CSUM_SIZE] = buf[offsets::CSUM..offsets::CSUM + CSUM_SIZE]
            .try_into()
            .expect("32 bytes");
        let covered = &buf[CSUM_SIZE..SUPER_INFO_SIZE];
        if !csum_type.verify(covered, &csum) {
            return Err(Error::ChecksumMismatch {
                what: "superblock",
                offset: le64(buf, offsets::BYTENR),
            });
        }

        let incompat_flags = le64(buf, offsets::INCOMPAT_FLAGS);
        refuse_unreadable_incompat(incompat_flags)?;

        let sys_len = le32(buf, offsets::SYS_CHUNK_ARRAY_SIZE) as usize;
        if sys_len > SYS_CHUNK_ARRAY_SIZE {
            return Err(malformed(format!(
                "sys_chunk_array_size {sys_len} exceeds the {SYS_CHUNK_ARRAY_SIZE}-byte array"
            )));
        }
        let sys_chunk_array =
            buf[offsets::SYS_CHUNK_ARRAY..offsets::SYS_CHUNK_ARRAY + sys_len].to_vec();

        let label_raw = &buf[offsets::LABEL..offsets::LABEL + LABEL_SIZE];
        let label_end = label_raw.iter().position(|&b| b == 0).unwrap_or(LABEL_SIZE);
        let label = String::from_utf8_lossy(&label_raw[..label_end]).into_owned();

        let fsid = uuid_at(buf, offsets::FSID);
        let sb = Superblock {
            csum,
            fsid,
            bytenr: le64(buf, offsets::BYTENR),
            flags: le64(buf, offsets::FLAGS),
            generation: le64(buf, offsets::GENERATION),
            root: le64(buf, offsets::ROOT),
            chunk_root: le64(buf, offsets::CHUNK_ROOT),
            log_root: le64(buf, offsets::LOG_ROOT),
            log_root_transid: le64(buf, offsets::LOG_ROOT_TRANSID),
            total_bytes: le64(buf, offsets::TOTAL_BYTES),
            bytes_used: le64(buf, offsets::BYTES_USED),
            root_dir_objectid: le64(buf, offsets::ROOT_DIR_OBJECTID),
            num_devices: le64(buf, offsets::NUM_DEVICES),
            sectorsize: le32(buf, offsets::SECTORSIZE),
            nodesize: le32(buf, offsets::NODESIZE),
            leafsize: le32(buf, offsets::LEAFSIZE),
            stripesize: le32(buf, offsets::STRIPESIZE),
            sys_chunk_array_size: sys_len as u32,
            chunk_root_generation: le64(buf, offsets::CHUNK_ROOT_GENERATION),
            compat_flags: le64(buf, offsets::COMPAT_FLAGS),
            compat_ro_flags: le64(buf, offsets::COMPAT_RO_FLAGS),
            incompat_flags,
            csum_type,
            root_level: buf[offsets::ROOT_LEVEL],
            chunk_root_level: buf[offsets::CHUNK_ROOT_LEVEL],
            log_root_level: buf[offsets::LOG_ROOT_LEVEL],
            dev_item: DevItem::parse(&buf[offsets::DEV_ITEM..offsets::DEV_ITEM + DEV_ITEM_SIZE])?,
            label,
            cache_generation: le64(buf, offsets::CACHE_GENERATION),
            uuid_tree_generation: le64(buf, offsets::UUID_TREE_GENERATION),
            // The on-disk field is written only when the METADATA_UUID
            // feature is enabled; without it the field is all zeros and
            // the metadata UUID *is* the fsid. Verified against real
            // media: mkfs.btrfs leaves 0x23b zeroed, while the reference
            // tooling reports metadata_uuid identical to fsid. Returning
            // the raw zeros here would make every tree node fail its
            // identity check on an ordinary filesystem.
            metadata_uuid: if le64(buf, offsets::INCOMPAT_FLAGS) & incompat::METADATA_UUID != 0 {
                uuid_at(buf, offsets::METADATA_UUID)
            } else {
                fsid
            },
            nr_global_roots: le64(buf, offsets::NR_GLOBAL_ROOTS),
            sys_chunk_array,
        };

        sb.validate()?;
        Ok(sb)
    }

    /// Parse a superblock that was read from `offset`, and additionally
    /// require the copy to agree that it belongs there.
    ///
    /// Btrfs writes its own physical offset into `bytenr`, which makes a
    /// misdirected read detectable independently of the checksum — a
    /// stale-but-intact superblock from a previous filesystem at a
    /// different offset has a perfectly good digest.
    pub fn parse_at(buf: &[u8], offset: u64) -> Result<Self> {
        let sb = Self::parse(buf)?;
        if sb.bytenr != offset {
            return Err(Error::BlockIdentityMismatch {
                what: "superblock",
                expected: offset,
                found: sb.bytenr,
            });
        }
        Ok(sb)
    }

    /// Structural sanity checks.
    ///
    /// Btrfs has fewer redundant `log2` companions than XFS does, so the
    /// checks here lean on cross-field agreement instead: `bytenr` must
    /// name a real mirror offset, tree roots must be sector-aligned,
    /// `dev_item.fsid` must agree with the volume's own identity, and the
    /// size fields must bracket each other. Each of those fails loudly if
    /// a field is read at the wrong offset.
    fn validate(&self) -> Result<()> {
        self.validate_geometry()?;
        self.validate_tree_roots()?;
        self.validate_sys_chunk_array()?;
        self.validate_device_identity()?;
        Ok(())
    }

    /// Sizes, counts and capacities: the numbers describing the device
    /// itself, independent of any tree it holds.
    fn validate_geometry(&self) -> Result<()> {
        let bad = |m: String| Err(malformed(m));
        if !SUPER_OFFSETS.contains(&self.bytenr) {
            return bad(format!(
                "bytenr {:#x} is not one of the superblock mirror offsets {SUPER_OFFSETS:#x?}",
                self.bytenr
            ));
        }
        if !self.sectorsize.is_power_of_two()
            || !(MIN_SECTORSIZE..=MAX_METADATA_BLOCKSIZE).contains(&self.sectorsize)
        {
            return bad(format!(
                "sectorsize {} is not a power of two in [{MIN_SECTORSIZE}, {MAX_METADATA_BLOCKSIZE}]",
                self.sectorsize
            ));
        }
        if !self.nodesize.is_power_of_two()
            || self.nodesize < self.sectorsize
            || self.nodesize > MAX_METADATA_BLOCKSIZE
        {
            return bad(format!(
                "nodesize {} is not a power of two in [sectorsize {}, {MAX_METADATA_BLOCKSIZE}]",
                self.nodesize, self.sectorsize
            ));
        }
        // Modern kernels require stripesize == sectorsize exactly. Old
        // mkfs.btrfs wrote a hard-coded 4096 here, which agrees with the
        // rule for every volume that also uses 4 KiB sectors — i.e. for
        // essentially all of them. Flagged for cross-validation: if a
        // real 16 KiB-sector image ever trips this, relax it to a
        // power-of-two check.
        if self.stripesize != self.sectorsize {
            return bad(format!(
                "stripesize {} disagrees with sectorsize {}",
                self.stripesize, self.sectorsize
            ));
        }
        if self.num_devices == 0 {
            return bad("num_devices is zero".into());
        }
        if self.total_bytes == 0 {
            return bad("total_bytes is zero".into());
        }
        if !self.total_bytes.is_multiple_of(u64::from(self.sectorsize)) {
            return bad(format!(
                "total_bytes {} is not a multiple of sectorsize {}",
                self.total_bytes, self.sectorsize
            ));
        }
        if self.bytes_used > self.total_bytes {
            return bad(format!(
                "bytes_used {} exceeds total_bytes {}",
                self.bytes_used, self.total_bytes
            ));
        }
        Ok(())
    }

    /// The three tree roots the superblock points at: their depth must be
    /// within the format's maximum tree height, their addresses must be
    /// sector-aligned, and the two that are mandatory must be present.
    fn validate_tree_roots(&self) -> Result<()> {
        let bad = |m: String| Err(malformed(m));
        for (name, level) in [
            ("root_level", self.root_level),
            ("chunk_root_level", self.chunk_root_level),
            ("log_root_level", self.log_root_level),
        ] {
            if level >= MAX_LEVEL {
                return bad(format!(
                    "{name} {level} is at or above the maximum tree height {MAX_LEVEL}"
                ));
            }
        }
        for (name, addr) in [
            ("root", self.root),
            ("chunk_root", self.chunk_root),
            ("log_root", self.log_root),
        ] {
            if !addr.is_multiple_of(u64::from(self.sectorsize)) {
                return bad(format!(
                    "{name} {addr:#x} is not sectorsize-aligned ({})",
                    self.sectorsize
                ));
            }
        }
        if self.chunk_root == 0 {
            return bad("chunk_root is zero — nothing can be mapped without it".into());
        }
        if self.root == 0 {
            return bad("root tree address is zero".into());
        }
        Ok(())
    }

    /// The bootstrap chunk mapping embedded in the superblock. Without at
    /// least one usable entry the chunk tree cannot be located, and
    /// nothing else on the volume can be read.
    fn validate_sys_chunk_array(&self) -> Result<()> {
        let bad = |m: String| Err(malformed(m));
        // The system chunk array must be able to hold at least one
        // (key, chunk item, stripe) triple, otherwise the chunk tree is
        // unreachable. 17 (key) + 48 (chunk header) + 32 (one stripe)
        // = 97. The reference implementation makes the same check, its
        // chunk structure carrying one embedded stripe.
        const MIN_SYS_CHUNK_ARRAY: u32 = 17 + 48 + 32;
        if self.sys_chunk_array_size < MIN_SYS_CHUNK_ARRAY {
            return bad(format!(
                "sys_chunk_array_size {} cannot hold a single chunk mapping (need {MIN_SYS_CHUNK_ARRAY})",
                self.sys_chunk_array_size
            ));
        }
        Ok(())
    }

    /// The device's own record of which filesystem it belongs to.
    fn validate_device_identity(&self) -> Result<()> {
        let bad = |m: String| Err(malformed(m));
        // Every device carries the identity of the filesystem it belongs
        // to. Skipped while an fsid change is in flight or when this is a
        // seed device, because in those states the two legitimately
        // disagree.
        let identity_in_flux = self.flags
            & (super_flags::CHANGING_FSID | super_flags::CHANGING_FSID_V2 | super_flags::SEEDING)
            != 0;
        if !identity_in_flux && self.dev_item.fsid != self.node_uuid() {
            return bad("dev_item.fsid does not match the volume's own UUID".into());
        }
        Ok(())
    }

    /// The UUID stamped into tree node headers.
    ///
    /// Equals [`Self::fsid`] unless the `METADATA_UUID` incompatible
    /// feature is set, in which case the volume's visible UUID and the
    /// one written into metadata have been deliberately decoupled (that
    /// is the whole point of the feature — it lets `fsid` be changed
    /// without rewriting every tree block).
    pub fn node_uuid(&self) -> [u8; UUID_SIZE] {
        if self.incompat_flags & incompat::METADATA_UUID != 0 {
            self.metadata_uuid
        } else {
            self.fsid
        }
    }

    /// Whether the log tree holds changes not yet folded into the main
    /// trees. Reading the committed trees remains safe; the data is just
    /// slightly behind the last `fsync`.
    ///
    /// Gates: every mount refuses such a volume with `Error::DirtyLog`.
    pub fn has_dirty_log(&self) -> bool {
        self.log_root != 0
    }

    /// Whether this device is a read-only seed for another filesystem.
    ///
    /// Gates: `Filesystem::mount_rw` refuses a seed; it mounts read-only.
    pub fn is_seeding(&self) -> bool {
        self.flags & super_flags::SEEDING != 0
    }

    /// Whether the volume was flagged as having hit an error.
    ///
    /// An accessor for a consumer; the driver does not branch on it. This
    /// crate has nowhere to surface a warning, so whether to tell a user
    /// before a write is the caller's decision.
    pub fn has_error_flag(&self) -> bool {
        self.flags & super_flags::ERROR != 0
    }

    /// Whether this image came from a metadata-only dump tool. Such an
    /// image has no data extents and cannot be mounted.
    ///
    /// Gates: every mount refuses one.
    pub fn is_metadump(&self) -> bool {
        self.flags & (super_flags::METADUMP | super_flags::METADUMP_V2) != 0
    }

    /// Whether file holes are implicit rather than recorded as extents.
    ///
    /// An accessor; the reader handles both layouts without branching.
    pub fn has_no_holes(&self) -> bool {
        self.incompat_flags & incompat::NO_HOLES != 0
    }

    /// Whether metadata back-references use the compact "skinny" form.
    ///
    /// An accessor; `extent_write` reads the incompat bit itself.
    pub fn has_skinny_metadata(&self) -> bool {
        self.incompat_flags & incompat::SKINNY_METADATA != 0
    }

    /// Whether data and metadata share block groups.
    ///
    /// An accessor; the reader handles both layouts without branching, as
    /// the `mixed` fixture shows.
    pub fn has_mixed_groups(&self) -> bool {
        self.incompat_flags & incompat::MIXED_GROUPS != 0
    }

    /// Whether a free-space tree (space_cache v2) is present and valid.
    ///
    /// An accessor; the write path consults the compat_ro bits directly.
    pub fn has_free_space_tree(&self) -> bool {
        self.compat_ro_flags & (compat_ro::FREE_SPACE_TREE | compat_ro::FREE_SPACE_TREE_VALID)
            == (compat_ro::FREE_SPACE_TREE | compat_ro::FREE_SPACE_TREE_VALID)
    }

    /// Whether block group items live in their own tree.
    ///
    /// Gates, through the compat_ro bit: `mount_rw` refuses such a volume
    /// (`refuse_unmaintained_compat_ro`, #72), because the write path
    /// updates block group items in the extent tree. Reading the block
    /// groups (`Filesystem::block_groups`, and `fsck.btrfs` through it)
    /// follows the bit to the block group tree (#270).
    pub fn has_block_group_tree(&self) -> bool {
        self.compat_ro_flags & compat_ro::BLOCK_GROUP_TREE != 0
    }
}

/// The superblock a mount should use: the valid copy with the highest
/// `generation`, and which copy that was (0 is the primary).
///
/// EVERY COPY IS READ (#90). Every read used copy 0 alone, so a volume
/// whose primary at 64 KiB was damaged -- by a stray `dd`, a partitioner,
/// or a commit of this driver's interrupted after copy 0 -- could not be
/// mounted, though the commit path writes all three. Reading every copy
/// rather than stopping at the first that parses is what lets a torn
/// commit's newer copy win over an older one; ties go to the lower copy.
///
/// A copy past the end of the device is ABSENT, not damaged: a volume
/// under 256 GiB has no copy 2, and one under 64 MiB has only copy 0.
///
/// AN UNREADABLE INCOMPAT FEATURE IS NOT DAMAGE (#447). A copy whose
/// checksum verifies and which names its own offset is intact, and when
/// it declares an incompatible feature this driver cannot read, the
/// volume uses that feature. Passing over it as damaged let the
/// read-only mount fall back to an older mirror and read the volume as
/// though the feature were not there, which is exactly what incompat
/// forbids. Every mount is refused instead, by
/// [`refuse_unreadable_incompat`]'s message.
///
/// # Errors
///
/// [`Error::UnsupportedFeature`] when a zoned superblock log is detected
/// at zero, or an intact copy declares an incompatible feature this
/// driver cannot read. Otherwise, when no copy
/// is valid, the primary's own error -- the one a caller would have seen
/// before.
pub fn read_superblock(dev: &dyn crate::device::BlockRead) -> Result<(Superblock, usize)> {
    let size = dev.size_bytes();
    // Real zoned devices keep a superblock log starting at zero. An
    // image has no zone write pointers, so a fixed-offset copy could be
    // stale even when its checksum verifies. Inspect the first record
    // before selecting any conventional mirror (#454). Its geometry is
    // deliberately not parsed as a conventional superblock.
    // Format reference: https://btrfs.readthedocs.io/en/latest/Zoned-mode.html
    if size >= SUPER_INFO_SIZE as u64 {
        let mut log = [0u8; SUPER_INFO_SIZE];
        dev.read_at(0, &mut log)?;
        if log[offsets::MAGIC..offsets::MAGIC + BTRFS_MAGIC.len()] == BTRFS_MAGIC
            && le64(&log, offsets::INCOMPAT_FLAGS) & incompat::ZONED != 0
            && ChecksumType::from_raw(le16(&log, offsets::CSUM_TYPE))
                .is_ok_and(|kind| kind.verify(&log[CSUM_SIZE..], &log[..CSUM_SIZE]))
        {
            return Err(Error::UnsupportedFeature(
                "this zoned device uses a superblock log; its latest superblock cannot be \
                 located without zone write pointers, so fixed-offset copies cannot be read"
                    .into(),
            ));
        }
    }
    let mut best: Option<(Superblock, usize)> = None;
    let mut primary_error = None;
    for (copy, &offset) in SUPER_OFFSETS.iter().enumerate() {
        if offset.saturating_add(SUPER_INFO_SIZE as u64) > size {
            continue;
        }
        let mut buf = vec![0u8; SUPER_INFO_SIZE];
        let parsed = dev
            .read_at(offset, &mut buf)
            .map_err(Error::from)
            .and_then(|()| Superblock::parse_at(&buf, offset));
        match parsed {
            Ok(sb) => {
                if best
                    .as_ref()
                    .is_none_or(|(b, _)| sb.generation > b.generation)
                {
                    best = Some((sb, copy));
                }
            }
            // `parse` checks the incompat bits only after the magic and
            // the checksum, so this copy is intact; its `bytenr` is
            // checked here because `parse_at` never got that far.
            Err(e @ Error::UnsupportedFeature(_)) if le64(&buf, offsets::BYTENR) == offset => {
                return Err(e)
            }
            Err(e) if copy == 0 => primary_error = Some(e),
            Err(_) => {}
        }
    }
    best.ok_or_else(|| {
        primary_error.unwrap_or_else(|| {
            Error::BadSuperblock("the device is too small to hold a superblock".to_string())
        })
    })
}
