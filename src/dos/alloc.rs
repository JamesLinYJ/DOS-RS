//! Memory arena from `DOS/ALLOC.ASM`.
//!
//! A free block has owner 0, which is `arena_owner_system`. The signature
//! byte is `M` until the last block, which is `Z`. `$ALLOC` is first-fit.
//! The returned segment is the header plus one paragraph.

const SIG_M: u8 = b'M';
const SIG_Z: u8 = b'Z';
const PDB: u16 = 0x00a0;
const NOT_ENOUGH: u16 = 8;
const INVALID_BLOCK: u16 = 9;

#[derive(Clone, Copy)]
struct Item {
    live: bool,
    seg: u16,
    size: u16,
    owner: u16,
    sig: u8,
}

const SLOTS: usize = 8;

static mut ITEMS: [Item; SLOTS] = [Item {
    live: false,
    seg: 0,
    size: 0,
    owner: 0,
    sig: 0,
}; SLOTS];
static mut READY: bool = false;

fn ensure() {
    unsafe {
        if READY {
            return;
        }
        ITEMS[0] = Item {
            live: true,
            seg: 0,
            size: 16,
            owner: 0,
            sig: SIG_Z,
        };
        let mut i = 1usize;
        while i < SLOTS {
            ITEMS[i].live = false;
            i += 1;
        }
        READY = true;
    }
}

fn count() -> usize {
    let mut n = 0usize;
    unsafe {
        while n < SLOTS && ITEMS[n].live {
            n += 1;
        }
    }
    n
}

fn coalesce(index: usize) {
    loop {
        let n = count();
        if index + 1 >= n {
            return;
        }
        unsafe {
            if ITEMS[index].sig == SIG_Z {
                return;
            }
            if ITEMS[index + 1].owner != 0 {
                return;
            }
            ITEMS[index].size = ITEMS[index]
                .size
                .wrapping_add(ITEMS[index + 1].size)
                .wrapping_add(1);
            ITEMS[index].sig = ITEMS[index + 1].sig;
        }
        let mut j = index + 1;
        while j + 1 < n {
            unsafe { ITEMS[j] = ITEMS[j + 1] }
            j += 1;
        }
        unsafe { ITEMS[n - 1].live = false }
    }
}

/// Size of the biggest free block, in paragraphs. `$ALLOC` of `0xFFFF` returns this in BX.
pub fn largest_free() -> u16 {
    ensure();
    let mut max = 0u16;
    let n = count();
    let mut index = 0usize;
    while index < n {
        unsafe {
            if ITEMS[index].owner == 0 && ITEMS[index].size > max {
                max = ITEMS[index].size;
            }
        }
        index += 1;
    }
    max
}

/// AH=48h. `Ok` is the memory segment. `Err` is the code and the largest block.
pub fn alloc(request: u16) -> Result<u16, (u16, u16)> {
    ensure();
    let mut max = 0u16;
    let mut index = 0usize;
    let n = count();
    while index < n {
        unsafe {
            if ITEMS[index].owner == 0 {
                coalesce(index);
                let size = ITEMS[index].size;
                if size > max {
                    max = size;
                }
                if request <= size {
                    return Ok(split(index, request));
                }
            }
        }
        index += 1;
    }
    let _ = crate::dos::ms_code::sys_ret_err(NOT_ENOUGH);
    Err((NOT_ENOUGH, max))
}

fn split(index: usize, request: u16) -> u16 {
    unsafe {
        let seg = ITEMS[index].seg;
        let size = ITEMS[index].size;
        if size != request {
            let leftover = size - request;
            let new_size = leftover - 1;
            let new_seg = seg + request + 1;
            let old_sig = ITEMS[index].sig;
            let n = count();
            let mut j = n;
            while j > index + 1 {
                ITEMS[j] = ITEMS[j - 1];
                j -= 1;
            }
            ITEMS[index + 1] = Item {
                live: true,
                seg: new_seg,
                size: new_size,
                owner: 0,
                sig: old_sig,
            };
            ITEMS[index].sig = SIG_M;
            ITEMS[index].size = request;
        }
        ITEMS[index].owner = PDB;
        seg + 1
    }
}

/// AH=49h. `es` is the memory segment, one paragraph above the header.
pub fn free(extra_segment: u16) -> u16 {
    ensure();
    let n = count();
    let mut index = 0usize;
    while index < n {
        unsafe {
            if ITEMS[index].seg + 1 == extra_segment {
                ITEMS[index].owner = 0;
                return 0;
            }
        }
        index += 1;
    }
    INVALID_BLOCK
}

pub fn show() {
    let first = alloc(4).unwrap_or(0);
    let second = alloc(4).unwrap_or(0);
    let _ = free(first);
    let again = alloc(4).unwrap_or(0);
    let (code, biggest) = match alloc(100) {
        Err(pair) => pair,
        Ok(_) => (0, 0),
    };
    crate::bios::mscon::con_write_bytes(b"ALLOC ");
    put_hex16(first);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(second);
    crate::bios::mscon::con_write_bytes(b" FIT");
    put_hex16(again);
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(code as u8);
    crate::bios::mscon::con_write_bytes(b" BX");
    put_hex16(biggest);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
