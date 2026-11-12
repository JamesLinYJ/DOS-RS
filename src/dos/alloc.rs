//! Memory arena from `DOS/ALLOC.ASM`.
//!
//! A free block has owner 0, which is `arena_owner_system`. The active block list
//! tracks the DOS headers in segment order. Allocation uses first fit.
//! The returned segment is the header plus one paragraph.

const PDB: u16 = 0x00a0;
const NOT_ENOUGH: u16 = 8;
const INVALID_BLOCK: u16 = 9;
const PARAGRAPHS: u16 = 16;
const SLOTS: usize = PARAGRAPHS as usize + 1;

#[derive(Clone, Copy)]
struct Block {
    header: u16,
    size: u16,
    owner: u16,
}
struct Arena {
    blocks: [Block; SLOTS],
    used: usize,
}
static ARENA: crate::kernel_cell::KernelCell<Arena> = crate::kernel_cell::KernelCell::new(Arena {
    blocks: [Block {
        header: 0,
        size: PARAGRAPHS,
        owner: 0,
    }; SLOTS],
    used: 1,
});

impl Arena {
    fn merge_next(&mut self, i: usize) {
        if i + 1 < self.used && self.blocks[i + 1].owner == 0 {
            self.blocks[i].size += 1 + self.blocks[i + 1].size;
            self.blocks.copy_within(i + 2..self.used, i + 1);
            self.used -= 1;
        }
    }
    fn largest(&self) -> u16 {
        self.blocks[..self.used]
            .iter()
            .filter(|b| b.owner == 0)
            .map(|b| b.size)
            .max()
            .unwrap_or(0)
    }
    fn alloc(&mut self, request: u16) -> Option<u16> {
        let i = self.blocks[..self.used]
            .iter()
            .position(|b| b.owner == 0 && b.size >= request)?;
        let block = self.blocks[i];
        if block.size > request {
            self.blocks.copy_within(i + 1..self.used, i + 2);
            self.used += 1;
            self.blocks[i + 1] = Block {
                header: block.header + request + 1,
                size: block.size - request - 1,
                owner: 0,
            };
        }
        self.blocks[i].size = request;
        self.blocks[i].owner = PDB;
        Some(block.header + 1)
    }
}

pub fn largest_free() -> u16 {
    ARENA.borrow().largest()
}

pub fn alloc(request: u16) -> Result<u16, (u16, u16)> {
    let mut arena = ARENA.borrow();
    if let Some(segment) = arena.alloc(request) {
        return Ok(segment);
    }
    let _ = crate::dos::ms_code::sys_ret_err(NOT_ENOUGH);
    Err((NOT_ENOUGH, arena.largest()))
}

pub fn free(segment: u16) -> u16 {
    let mut arena = ARENA.borrow();
    let Some(i) = arena.blocks[..arena.used]
        .iter()
        .position(|b| b.owner == PDB && b.header + 1 == segment)
    else {
        return INVALID_BLOCK;
    };
    arena.blocks[i].owner = 0;
    arena.merge_next(i);
    if i > 0 && arena.blocks[i - 1].owner == 0 {
        arena.merge_next(i - 1);
    }
    0
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
