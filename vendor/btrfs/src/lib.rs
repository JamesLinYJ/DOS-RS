//! Bare-metal port of the upstream Btrfs on-disk and transaction implementation.
#![no_std]
#[macro_use]
extern crate alloc;
pub use dosrs_block as device;
mod crc32c;
use core::time::Duration;
static CLOCK: spin::Mutex<fn() -> Duration> = spin::Mutex::new(|| Duration::ZERO);
pub fn set_clock(read: fn() -> Duration) { *CLOCK.lock() = read; }
static UUID: spin::Mutex<fn() -> Option<[u8;16]>> = spin::Mutex::new(|| None);
pub fn set_uuid_source(read:fn()->Option<[u8;16]>) { *UUID.lock()=read; }
fn new_uuid()->Result<[u8;16]>{let read=*UUID.lock();read().ok_or_else(|| Error::UnsupportedFeature("no UUID source for subvolume creation".into()))}
fn clock() -> Duration { let read = *CLOCK.lock(); read() }
pub mod attrs;
pub mod backref;
pub mod block_group;
pub mod btree;
pub mod chunk;
pub mod commit;
pub mod compression;
pub mod cow_write;
pub mod csum;
pub mod dir;
pub mod error;
pub mod extent_write;
pub mod fs;
pub mod inode;
pub mod leaf_edit;
pub mod log_tree;
pub mod namespace;
pub mod raid56;
pub mod subvol;
pub mod super_write;
pub mod superblock;
pub mod transaction;
pub mod tree_write;
pub mod truncate;
pub mod verity;
pub mod write;
pub mod xattr;

pub use chunk::{Chunk, ChunkMap, ChunkProfile, Mapping};
pub use error::{Error, Result};
pub use fs::Filesystem;
pub use superblock::{ChecksumType, Superblock};
pub use xattr::XattrEntry;
