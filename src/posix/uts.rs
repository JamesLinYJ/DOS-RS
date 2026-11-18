//! UTS data for the running kernel. uname uses six 65-byte fields on both musl ABIs.
use crate::{arch::user::AddressSpace, kernel_cell::KernelCell};

const FIELD: usize = 65;
const INITIAL: [u8; FIELD] = {
    let mut buf = [0; FIELD];
    let name = b"dos-rs";
    let mut i = 0;
    while i < name.len() {
        buf[i] = name[i];
        i += 1;
    }
    buf
};
static NODENAME: KernelCell<[u8; FIELD]> = KernelCell::new(INITIAL);

#[cfg(target_arch = "x86")]
const MACHINE: &[u8] = b"i686";
#[cfg(target_arch = "x86_64")]
const MACHINE: &[u8] = b"x86_64";

pub(super) fn uname(image: &mut AddressSpace, dest: usize) -> Result<usize, isize> {
    let mut result = [0u8; FIELD * 6];
    let name = NODENAME.borrow();
    let profile: &[u8] = if cfg!(debug_assertions) {
        b"debug"
    } else {
        b"release"
    };
    let values: [&[u8]; 6] = [
        b"DOS-RS",
        &*name,
        env!("CARGO_PKG_VERSION").as_bytes(),
        profile,
        MACHINE,
        b"",
    ];
    for (slot, value) in result.chunks_mut(FIELD).zip(values) {
        let len = value.len().min(FIELD - 1);
        slot[..len].copy_from_slice(&value[..len]);
    }
    drop(name);
    image.write(dest, &result).map_err(|_| -14isize)?;
    Ok(0)
}

pub(super) fn sethostname(
    image: &AddressSpace,
    uid: u32,
    source: usize,
    len: usize,
) -> Result<usize, isize> {
    if uid != 0 {
        return Err(-1);
    }
    if len >= FIELD {
        return Err(-22);
    }
    let mut new = [0; FIELD];
    if len != 0 {
        image.read(source, &mut new[..len]).map_err(|_| -14isize)?;
    }
    *NODENAME.borrow() = new;
    Ok(0)
}
