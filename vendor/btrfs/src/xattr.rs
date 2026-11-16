//! Extended attributes (`XATTR_ITEM`).
//!
//! # Where they live
//!
//! An attribute is filed under the key `(ino, 24, hash)`, where `hash`
//! is [`crate::dir::name_hash`] of the *fully-qualified* name — `user.colour`, not
//! `colour`. Btrfs stores no namespace table and no prefix encoding: the
//! name on disk is the name a caller asks for, prefix included. That
//! makes this driver's job smaller than its siblings', where ext4 packs
//! the prefix into a one-byte index and EROFS keeps a dictionary of
//! long prefixes shared across the image.
//!
//! # Why an item is a list
//!
//! The key's offset is a hash, so two names on the same inode can share
//! a key. When they do, their records are concatenated inside that one
//! item, exactly as colliding directory entries are. Reading only the
//! first record would drop the second attribute silently — and silently
//! is the problem: a caller cannot tell a file with one attribute from a
//! file whose second attribute this driver could not see.
//!
//! Collisions are not hypothetical and they are not rare enough to
//! ignore: a 32-bit hash over a few hundred attribute names on one inode
//! is a birthday problem, and a name can be chosen to collide on
//! purpose. `tests/xattr_oracle.rs` sets two names that do — found by
//! searching the hash — and requires both to come back.
//!
//! # The record
//!
//! Same `struct btrfs_dir_item` the directory entries use, laid out at
//! [`crate::dir`]. For an attribute the interesting fields are the name
//! and the `data_len` bytes that follow it; the `location` key is
//! unused, and the `type` byte is [`ftype::XATTR`].
//!
//! # Read only
//!
//! Nothing here writes. Setting an attribute means inserting into a tree
//! that a transaction has to commit, which is a different piece of work
//! from decoding one.

use alloc::{vec::Vec, string::String};
use crate::dir::{ftype, parse_items};
use crate::error::{Error, Result};

/// One extended attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XattrEntry {
    /// The fully-qualified name, exactly as stored — namespace prefix
    /// included, not NUL-terminated, and not required to be UTF-8.
    pub name: Vec<u8>,
    /// The raw value. May be empty: a zero-length value is a real thing
    /// to store, and is not the same as the attribute being absent.
    pub value: Vec<u8>,
}

/// Parse the sequence of attribute records packed into one item's data.
///
/// # Errors
///
/// As `parse_items`, plus [`Error::BadSuperblock`] if a record's
/// `type` byte is not [`ftype::XATTR`].
///
/// # Why the type byte is checked
///
/// It is redundant against a correct caller — this is only ever handed
/// the data of an item whose key type is already
/// [`crate::dir::XATTR_ITEM_KEY`] — and that is exactly why it is cheap to keep. If
/// it ever fires, the item being read is not the item that was meant,
/// and the alternative to noticing is returning a directory entry's
/// name as an attribute with an empty value. The oracle fixture
/// confirms the kernel does write `8` here. `XATTR_ITEM_KEY` is
/// [`crate::dir::XATTR_ITEM_KEY`].
pub fn parse_xattr_items(data: &[u8]) -> Result<Vec<XattrEntry>> {
    parse_items(data, "extended attribute item")?
        .into_iter()
        .map(|r| {
            if r.ftype != ftype::XATTR {
                return Err(Error::BadSuperblock(format!(
                    "extended attribute {:?} carries file type {} rather than \
                     {} — this is not an XATTR_ITEM",
                    String::from_utf8_lossy(r.name),
                    r.ftype,
                    ftype::XATTR
                )));
            }
            Ok(XattrEntry {
                name: r.name.to_vec(),
                value: r.value.to_vec(),
            })
        })
        .collect()
}
