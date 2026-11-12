//! Open and read from `DOS/HANDLE.ASM`, for the boot image.
//!
//! AH=3Dh returns handle 5 when the 8.3 name is in the root. AH=3Fh copies
//! the next bytes of that file. The file data starts at cluster 2, LBA 4.

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");

use crate::kernel_cell::KernelCell;

enum Backing {
    Owned,
    Image { start: usize },
}
struct OpenFile {
    index: usize,
    position: u32,
    size: u32,
    backing: Backing,
    dirty: bool,
}
static OPEN: KernelCell<Option<OpenFile>> = KernelCell::new(None);

pub fn open_asciiz(name: *const u8) -> Option<u16> {
    let mut raw = [0u8; 12];
    let mut len = 0usize;
    unsafe {
        while len < raw.len() {
            let byte = *name.add(len);
            if byte == 0 {
                break;
            }
            raw[len] = byte;
            len += 1;
        }
    }
    let packed = crate::dos::search::pack(&raw[..len])?;
    if let Some(index) = crate::dos::search::find_packed(&packed) {
        let ent = crate::dos::search::slot(index);
        if ent.dir {
            return None;
        }
        if ent.owned {
            bind_owned(index);
            return Some(5);
        }
        if ent.cluster >= 2 {
            let start = (4 + (ent.cluster as usize - 2)) * 512;
            if start < IMAGE.len() {
                arm_image(index, start, ent.size as usize);
                return Some(5);
            }
        }
    }
    let root = 3 * 512;
    let mut index = 0usize;
    while index < 16 {
        let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
        if entry[0] == 0 {
            break;
        }
        if entry[0] != 0xe5 && entry[..11].eq_ignore_ascii_case(&packed) {
            let cluster = u16::from_le_bytes([entry[26], entry[27]]) as usize;
            let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as usize;
            if cluster < 2 {
                return None;
            }
            let start = (4 + (cluster - 2)) * 512;
            if start >= IMAGE.len() {
                return None;
            }
            arm_image(index, start, size);
            return Some(5);
        }
        index += 1;
    }
    None
}

pub fn bind_owned(index: usize) {
    let size = u32::from(crate::dos::search::slot(index).data_len);
    *OPEN.borrow() = Some(OpenFile {
        index,
        size,
        backing: Backing::Owned,
        position: 0,
        dirty: false,
    });
}

fn arm_image(index: usize, start: usize, size: usize) {
    *OPEN.borrow() = Some(OpenFile {
        index,
        size: size as u32,
        backing: Backing::Image { start },
        position: 0,
        dirty: false,
    });
}

pub fn opened_index(handle: u16) -> Option<usize> {
    if handle != 5 {
        return None;
    }
    OPEN.borrow().as_ref().map(|file| file.index)
}

pub fn write(handle: u16, src: *const u8, count: usize) -> usize {
    let mut guard = OPEN.borrow();
    let Some(file) = guard.as_mut() else { return 0 };
    if handle != 5 || !matches!(file.backing, Backing::Owned) || src.is_null() {
        return 0;
    }
    let mut ent = crate::dos::search::slot(file.index);
    let pos = file.position as usize;
    let Some(room) = ent.data.get_mut(pos..) else {
        return 0;
    };
    let n = room.len().min(count);
    if n == 0 {
        return 0;
    }
    // The native DOS ABI supplies at least count readable bytes.
    unsafe { core::ptr::copy(src, room.as_mut_ptr(), n) };
    file.position += n as u32;
    ent.data_len = ent.data_len.max(file.position as u8);
    file.size = u32::from(ent.data_len);
    ent.size = u16::from(ent.data_len);
    file.dirty = true;
    crate::dos::search::set_slot(file.index, ent);
    n
}

pub fn seek(handle: u16, method: u8, low: u16, high: u16) -> Result<u32, u16> {
    let mut guard = OPEN.borrow();
    let file = guard.as_mut().filter(|_| handle == 5).ok_or(6u16)?;
    let offset = (u32::from(high) << 16) | u32::from(low);
    file.position = match method {
        0 => offset,
        1 => file.position.wrapping_add(offset),
        2 => file.size.wrapping_add(offset),
        _ => return Err(1),
    };
    Ok(file.position)
}

pub fn close(handle: u16) -> u16 {
    if handle != 5 {
        return 6;
    }
    let Some(file) = OPEN.borrow().take() else {
        return 6;
    };
    if file.dirty {
        crate::dos::close::commit(file.index);
    }
    0
}

fn copy_out(data: &[u8], offset: usize, dst: *mut u8, count: usize) -> usize {
    let Some(rest) = data.get(offset..) else {
        return 0;
    };
    let n = count.min(rest.len());
    if n != 0 && !dst.is_null() {
        // The native DOS caller supplies room for count bytes at dst.
        unsafe { core::ptr::copy(rest.as_ptr(), dst, n) };
        n
    } else {
        0
    }
}

fn image_data(start: usize, size: usize) -> &'static [u8] {
    let data = IMAGE.get(start..).unwrap_or_default();
    &data[..data.len().min(size)]
}

pub fn read_at(name: &[u8; 11], offset: usize, dst: *mut u8, count: usize) -> usize {
    let Some(index) = crate::dos::search::find_packed(name) else {
        return 0;
    };
    let ent = crate::dos::search::slot(index);
    if ent.dir {
        return 0;
    }
    if ent.owned {
        return copy_out(&ent.data[..usize::from(ent.data_len)], offset, dst, count);
    }
    if ent.cluster < 2 {
        return 0;
    }
    let start = (4 + usize::from(ent.cluster - 2)) * 512;
    copy_out(image_data(start, usize::from(ent.size)), offset, dst, count)
}

pub fn read(handle: u16, dst: *mut u8, count: usize) -> usize {
    let mut guard = OPEN.borrow();
    let Some(file) = guard.as_mut().filter(|_| handle == 5) else {
        return 0;
    };
    let pos = file.position as usize;
    let n = match file.backing {
        Backing::Owned => {
            let ent = crate::dos::search::slot(file.index);
            copy_out(&ent.data[..usize::from(ent.data_len)], pos, dst, count)
        }
        Backing::Image { start } => {
            copy_out(image_data(start, file.size as usize), pos, dst, count)
        }
    };
    file.position += n as u32;
    n
}
