//! Btrfs chunk allocation. The chunk, device, block group and free-space records are committed together.
//! Transaction and stripe helpers are ported from the upstream balance implementation (see NOTICE).
use crate::chunk::{
    block_group as bg, key_type, objectid, ChunkProfile, DiskKey, DEFAULT_STRIPE_LEN,
};
use crate::{
    leaf_edit::OwnedItem,
    super_write::Commit,
    transaction::{DataWrite, ItemEdit},
    Error, Filesystem, Result,
};
use alloc::vec::Vec;

const PLAN_ROUNDS: usize = 64;
const MIB: u64 = 1 << 20;

/// `BTRFS_DEV_EXTENT_KEY`.
const DEV_EXTENT_KEY: u8 = 204;

/// Byte offsets within `struct btrfs_dev_extent`.
mod dev_extent {
    /// The stripe's length on the device.
    pub const LENGTH: usize = 24;
    /// The chunk tree's UUID.
    pub const CHUNK_TREE_UUID: usize = 32;
    /// The whole item.
    pub const SIZE: usize = 48;
}

/// Where a device item's size is, in the chunk tree's copy.
const DEV_ITEM_TOTAL_BYTES: usize = 8;
/// Where a device item's `bytes_used` is, in the chunk tree's copy.
const DEV_ITEM_BYTES_USED: usize = 16;
/// A `BLOCK_GROUP_ITEM`: used, chunk objectid, flags.
const BLOCK_GROUP_ITEM_SIZE: usize = 24;

impl Filesystem {
    // Keep enough old metadata blocks free for publishing a replacement chunk tree.
    // A chunk becomes allocatable only after its superblock commit and remount.
    pub(crate) fn ensure_metadata_reserve(&mut self) -> Result<()> {
        if !self.can_grow() || !self.metadata_needs_chunk()? {
            return Ok(());
        }
        match self.grow_kind(bg::METADATA, MIB) {
            Err(Error::NoSpace) => Ok(()), // A final truncate or unlink may still release space.
            other => other,
        }
    }

    fn metadata_needs_chunk(&self) -> Result<bool> {
        let free: u64 = self
            .block_groups()?
            .iter()
            .filter(|g| g.holds_metadata() && self.block_group_is_writable(g))
            .map(|g| g.free())
            .sum();
        Ok(free < u64::from(self.sb.nodesize) * 96)
    }

    fn can_grow(&self) -> bool {
        self.is_writable()
            && self.sb.num_devices == 1
            && !self.sb.has_mixed_groups()
            && self.sb.has_free_space_tree()
    }
    fn growth_profile(&self, kind: u64) -> Result<u64> {
        let mut flags = None;
        for g in self
            .block_groups()?
            .into_iter()
            .filter(|g| g.flags & kind != 0 && self.block_group_is_writable(g))
        {
            if !matches!(
                ChunkProfile::from_type(g.flags)?,
                ChunkProfile::Single | ChunkProfile::Dup
            ) || flags.is_some_and(|old| old != g.flags)
            {
                return Err(Error::NoSpace);
            }
            flags = Some(g.flags);
        }
        flags.ok_or(Error::NoSpace)
    }
    pub(crate) fn grow_data(&mut self, requested: u64) -> Result<()> {
        self.grow_kind(bg::DATA, requested)
    }
    fn grow_kind(&mut self, kind: u64, requested: u64) -> Result<()> {
        if !self.can_grow() {
            return Err(Error::NoSpace);
        }
        let flags = self.growth_profile(kind)?;
        let minimum = requested
            .max(1)
            .checked_next_multiple_of(MIB)
            .ok_or(Error::NoSpace)?;
        for size in chunk_sizes(kind, minimum) {
            match self.allocate_chunk(flags, size) {
                Ok(_) => return Ok(()),
                Err(Error::NoSpace) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(Error::NoSpace)
    }

    // Only the allocator's MiB-aligned, unoccupied ranges (after reserving each superblock
    // stripe row) can be contributed to a new data block group.
    pub(crate) fn expandable_data_bytes(&self) -> Result<u64> {
        if !self.can_grow() {
            return Ok(0);
        }
        let flags = match self.growth_profile(bg::DATA) {
            Ok(f) => f,
            Err(Error::NoSpace) => return Ok(0),
            Err(e) => return Err(e),
        };
        let copies = copies_on_device(flags);
        let devid = self.sb.dev_item.devid;
        let (_, item) = self.dev_item(devid)?;
        let total = le64(&item, DEV_ITEM_TOTAL_BYTES);
        let used: Vec<_> = self
            .dev_extents()?
            .iter()
            .filter(|e| e.devid == devid)
            .map(|e| (e.physical, e.length))
            .collect();
        let gaps = physical_gaps(&used, total);
        let free: u64 = gaps.iter().map(|(a, b)| b - a).sum();
        // The next write allocates its metadata chunk first. Those stripes cannot
        // also be reported as space for new file data, even before the commit.
        let mut reserved = 0;
        if self.metadata_needs_chunk()? {
            match self.growth_profile(bg::METADATA) {
                Ok(profile) => {
                    let copies = copies_on_device(profile);
                    if let Some(size) = chunk_sizes(bg::METADATA, MIB).find(|&size| {
                        gaps.iter().map(|(a, b)| (b - a) / size).sum::<u64>() >= copies
                    }) {
                        reserved = copies * size;
                    }
                }
                Err(Error::NoSpace) => (),
                Err(e) => return Err(e),
            }
        }
        Ok(free.saturating_sub(reserved) / MIB / copies * MIB)
    }

    /// Allocate a single or DUP chunk of `length` logical bytes and
    /// `flags`, with its empty block group, in one committed transaction,
    /// and return its logical address. DUP takes two stripes on this device.
    fn allocate_chunk(&mut self, flags: u64, length: u64) -> Result<u64> {
        let copies = match ChunkProfile::from_type(flags)? {
            ChunkProfile::Single => 1,
            ChunkProfile::Dup => 2,
            other => {
                return Err(Error::UnsupportedFeature(format!(
                    "allocating a {} chunk is not implemented",
                    other.name()
                )))
            }
        };
        // After every chunk there is, as the kernel places a new one.
        let logical = self
            .map
            .chunks()
            .iter()
            .map(|c| c.logical_end())
            .max()
            .unwrap_or(0);

        logical.checked_add(length).ok_or(Error::NoSpace)?;
        let extents = self.dev_extents()?;
        let devid = self.sb.dev_item.devid;
        if self.leaving == Some(devid) {
            return Err(Error::NoSpace);
        }
        let taken: Vec<_> = extents
            .iter()
            .filter(|e| e.devid == devid)
            .map(|e| (e.physical, e.length))
            .collect();
        let (dev_item_key, mut dev_item) = self.dev_item(devid)?;
        let total = le64(&dev_item, DEV_ITEM_TOTAL_BYTES);
        let stripes = place_stripes(&taken, total, length, copies).ok_or(Error::NoSpace)?;
        let chunk_tree_uuid = extents
            .first()
            .map(|e| e.chunk_tree_uuid)
            .ok_or_else(|| Error::UnsupportedFeature("the device tree has no extents".into()))?;
        let used = length
            .checked_mul(copies as u64)
            .and_then(|allocated| le64(&dev_item, DEV_ITEM_BYTES_USED).checked_add(allocated))
            .ok_or_else(|| {
                Error::UnsupportedFeature(format!("device {devid}'s bytes used overflow"))
            })?;
        dev_item[DEV_ITEM_BYTES_USED..DEV_ITEM_BYTES_USED + 8].copy_from_slice(&used.to_le_bytes());

        let dev_uuid = crate::superblock::DevItem::parse(&dev_item)?.uuid;
        let stripe_list: Vec<(u64, u64, [u8; 16])> =
            stripes.iter().map(|&p| (devid, p, dev_uuid)).collect();
        let mut tree_edits = vec![
            (
                objectid::CHUNK_TREE,
                ItemEdit::Put(OwnedItem {
                    key: dev_item_key,
                    data: dev_item,
                }),
            ),
            (
                objectid::CHUNK_TREE,
                ItemEdit::Put(OwnedItem {
                    key: DiskKey {
                        objectid: objectid::FIRST_CHUNK_TREE,
                        key_type: key_type::CHUNK_ITEM,
                        offset: logical,
                    },
                    data: chunk_item(length, flags, self.sb.sectorsize, &stripe_list),
                }),
            ),
        ];
        for &physical in &stripes {
            tree_edits.push((
                objectid::DEV_TREE,
                ItemEdit::Put(OwnedItem {
                    key: DiskKey {
                        objectid: devid,
                        key_type: DEV_EXTENT_KEY,
                        offset: physical,
                    },
                    data: dev_extent_item(logical, length, chunk_tree_uuid),
                }),
            ));
        }
        // An empty group: nothing used, in the chunk tree's first chunk
        // objectid, of the chunk's type.
        let mut group = Vec::with_capacity(BLOCK_GROUP_ITEM_SIZE);
        group.extend_from_slice(&0u64.to_le_bytes());
        group.extend_from_slice(&objectid::FIRST_CHUNK_TREE.to_le_bytes());
        group.extend_from_slice(&flags.to_le_bytes());
        tree_edits.push((
            self.block_group_tree_id(),
            ItemEdit::Put(OwnedItem {
                key: DiskKey {
                    objectid: logical,
                    key_type: key_type::BLOCK_GROUP_ITEM,
                    offset: length,
                },
                data: group,
            }),
        ));
        // Free, all of it: one run, recorded as an extent.
        let mut info = 1u32.to_le_bytes().to_vec();
        info.extend_from_slice(&0u32.to_le_bytes());
        for (key_type, data) in [
            (key_type::FREE_SPACE_INFO, info),
            (key_type::FREE_SPACE_EXTENT, Vec::new()),
        ] {
            tree_edits.push((
                objectid::FREE_SPACE_TREE,
                ItemEdit::Put(OwnedItem {
                    key: DiskKey {
                        objectid: logical,
                        key_type,
                        offset: length,
                    },
                    data,
                }),
            ));
        }
        self.commit_chunk_change(tree_edits, devid, used)?;
        Ok(logical)
    }
    /// Plan, render and commit `tree_edits`, with device `devid`'s chunks
    /// taking `bytes_used` in its superblock's device item as in the chunk
    /// tree's, then remount.
    fn commit_chunk_change(
        &mut self,
        tree_edits: Vec<(u64, ItemEdit)>,
        devid: u64,
        bytes_used: u64,
    ) -> Result<()> {
        let generation = self.sb.generation.checked_add(1).ok_or_else(|| {
            Error::UnsupportedFeature("the generation counter is exhausted".into())
        })?;
        let write = DataWrite {
            root: objectid::CHUNK_TREE,
            tree_edits,
            ..Default::default()
        };
        let dirty: Vec<u64> = {
            let reader = self.pool_reader();
            let tree = reader.tree();
            let mut leaves = alloc::collections::BTreeSet::new();
            for (t, edit) in &write.tree_edits {
                leaves.insert(
                    tree.descend(self.edit_root(*t)?, &edit.key())?
                        .header
                        .bytenr,
                );
            }
            leaves.into_iter().collect()
        };
        let plan = self.plan_transaction_closed_with(&dirty, &write, PLAN_ROUNDS)?;
        let blocks = self.render_plan_with(&plan, &write, generation)?;
        let root = self.planned_root(&plan).ok_or_else(|| {
            Error::UnsupportedFeature("the relocation's plan does not move the root tree".into())
        })?;
        let chunk_root = self.planned_chunk_root(&plan).ok_or_else(|| {
            Error::UnsupportedFeature("the relocation's plan does not move the chunk tree".into())
        })?;
        let sb_bytes_used = match plan.usage_delta(u64::from(self.sb.nodesize)) {
            0 => None,
            d => Some(
                u64::try_from(i128::from(self.sb.bytes_used) + d).map_err(|_| {
                    Error::UnsupportedFeature(format!(
                        "the superblock's {} bytes used cannot move by {d}",
                        self.sb.bytes_used
                    ))
                })?,
            ),
        };
        self.commit_bytes_used(
            &blocks,
            &Commit {
                generation,
                root,
                bytes_used: sb_bytes_used,
                chunk_root: Some(chunk_root),
                chunk_root_generation: Some(generation),
                invalidate_free_space_tree: false,
                ..Default::default()
            },
            devid,
            bytes_used,
        )?;
        let mut fresh = self.remount_rw()?;
        if let Some(held) = &self.subvolume_writes {
            // This is an intermediate commit. The pending file write still belongs
            // to the caller's subvolume, whose inode numbers can also exist at top level.
            fresh.fs_tree_root = fresh.tree_root(held.id)?;
            fresh.subvolume_writes = Some(held.clone());
        }
        *self = fresh;
        Ok(())
    }

    /// The chunk tree's item for device `devid`, and its key.
    fn dev_item(&self, devid: u64) -> Result<(DiskKey, Vec<u8>)> {
        let key = DiskKey {
            objectid: objectid::DEV_ITEMS,
            key_type: key_type::DEV_ITEM,
            offset: devid,
        };
        let item = self
            .pool_reader()
            .tree()
            .search(self.sb.chunk_root, &key)?
            .ok_or_else(|| {
                Error::UnsupportedFeature(format!("the chunk tree has no item for device {devid}"))
            })?
            .data;
        if item.len() < DEV_ITEM_BYTES_USED + 8 {
            return Err(Error::UnsupportedFeature(format!(
                "device {devid}'s item is {} bytes, shorter than a device item",
                item.len()
            )));
        }
        Ok((key, item))
    }

    /// Every device extent the device tree records.
    fn dev_extents(&self) -> Result<Vec<DevExtent>> {
        let root = self.tree_root(objectid::DEV_TREE)?;
        let reader = self.pool_reader();
        let tree = reader.tree();
        let mut out = Vec::new();
        let mut bad = None;
        tree.for_each(root, &mut |k: &DiskKey, d: &[u8]| {
            if k.key_type != DEV_EXTENT_KEY {
                return Ok(true);
            }
            if d.len() < dev_extent::SIZE {
                bad = Some(k.offset);
                return Ok(false);
            }
            let mut chunk_tree_uuid = [0u8; 16];
            chunk_tree_uuid
                .copy_from_slice(&d[dev_extent::CHUNK_TREE_UUID..dev_extent::CHUNK_TREE_UUID + 16]);
            out.push(DevExtent {
                devid: k.objectid,
                physical: k.offset,
                length: le64(d, dev_extent::LENGTH),
                chunk_tree_uuid,
            });
            Ok(true)
        })?;
        if let Some(at) = bad {
            return Err(Error::UnsupportedFeature(format!(
                "the device extent at {at} is shorter than a device extent"
            )));
        }
        Ok(out)
    }
}
fn copies_on_device(flags: u64) -> u64 {
    if flags & bg::DUP != 0 {
        2
    } else {
        1
    }
}

fn chunk_sizes(kind: u64, minimum: u64) -> impl Iterator<Item = u64> {
    let preferred = if kind == bg::DATA { 16 * MIB } else { 8 * MIB };
    core::iter::successors(Some(preferred.max(minimum)), move |&size| {
        (size > minimum).then(|| (size / 2 / MIB * MIB).max(minimum))
    })
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// One `DEV_EXTENT`: a stripe of a chunk, on a device.
#[derive(Debug, Clone, Copy)]
struct DevExtent {
    devid: u64,
    physical: u64,
    length: u64,
    chunk_tree_uuid: [u8; 16],
}

/// A `CHUNK_ITEM`'s body: `length` logical bytes of `flags`, over
/// `stripes`, each `(devid, physical offset, device UUID)`.
fn chunk_item(
    length: u64,
    flags: u64,
    sectorsize: u32,
    stripes: &[(u64, u64, [u8; 16])],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(48 + 32 * stripes.len());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&objectid::EXTENT_TREE.to_le_bytes()); // owner
    out.extend_from_slice(&DEFAULT_STRIPE_LEN.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(DEFAULT_STRIPE_LEN as u32).to_le_bytes()); // io_align
    out.extend_from_slice(&(DEFAULT_STRIPE_LEN as u32).to_le_bytes()); // io_width
    out.extend_from_slice(&sectorsize.to_le_bytes());
    out.extend_from_slice(&(stripes.len() as u16).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // sub_stripes
    for (devid, physical, uuid) in stripes {
        out.extend_from_slice(&devid.to_le_bytes());
        out.extend_from_slice(&physical.to_le_bytes());
        out.extend_from_slice(uuid);
    }
    out
}

/// A `DEV_EXTENT`'s body: a stripe of `length` bytes of the chunk at
/// `chunk_offset`, in the chunk tree.
fn dev_extent_item(chunk_offset: u64, length: u64, chunk_tree_uuid: [u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dev_extent::SIZE);
    out.extend_from_slice(&objectid::CHUNK_TREE.to_le_bytes());
    out.extend_from_slice(&objectid::FIRST_CHUNK_TREE.to_le_bytes());
    out.extend_from_slice(&chunk_offset.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&chunk_tree_uuid);
    debug_assert_eq!(out.len(), dev_extent::SIZE);
    out
}

/// Where `count` stripes of `len` bytes go on a device of `device_bytes`
/// whose extents `taken` already holds, each `(physical, length)`: the
/// first holes that fit, on MiB boundaries, past the first MiB, and
/// clear of every superblock copy -- so no allocation inside the chunk
/// has to step around one, and the free-space tree's one run covering
/// the group is true.
fn place_stripes(
    taken: &[(u64, u64)],
    device_bytes: u64,
    len: u64,
    count: usize,
) -> Option<Vec<u64>> {
    if len == 0 || !len.is_multiple_of(MIB) {
        return None;
    }
    let mut stripes = Vec::with_capacity(count);
    for (mut start, end) in physical_gaps(taken, device_bytes) {
        while end - start >= len && stripes.len() < count {
            stripes.push(start);
            start += len;
        }
        if stripes.len() == count {
            return Some(stripes);
        }
    }
    None
}

fn physical_gaps(taken: &[(u64, u64)], device_bytes: u64) -> Vec<(u64, u64)> {
    use crate::superblock::SUPER_OFFSETS;
    let end = device_bytes / MIB * MIB;
    let mut used: Vec<_> = taken
        .iter()
        .map(|&(start, len)| (start, start.saturating_add(len)))
        .chain(core::iter::once((0, MIB)))
        .chain(SUPER_OFFSETS.iter().map(|&sb| {
            (
                sb / MIB * MIB,
                (sb + crate::super_write::SUPERBLOCK_SIZE as u64).next_multiple_of(MIB),
            )
        }))
        .collect();
    used.sort_unstable();
    let mut here = 0u64;
    let mut gaps = Vec::new();
    for (start, stop) in used.into_iter().chain(core::iter::once((end, end))) {
        let upper = start.min(end) / MIB * MIB;
        if here < upper {
            gaps.push((here, upper));
        }
        here = here.max(stop.checked_next_multiple_of(MIB).unwrap_or(end));
        if here >= end {
            break;
        }
    }
    gaps
}
