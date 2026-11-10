//! Segment assertions from `DOS/SEGCHECK.ASM`.
//!
//! `SegCheck` compares DS and ES with SS, or with CS when flag bit 0 is set.
//! `DPBCheck`, `SFTCheck`, and `BUFCheck` walk a linked list and stop on
//! offset `FFFFh`. An SFT may also sit on the auxiliary stack or the rename
//! buffer. A disk critical-section entry that is still a `RET` skips the
//! owner and reference checks. A failed assertion is reported; the original
//! `CLI` / jump halt is not taken, so the boot proof can finish.

const USE_CODE: u8 = 0x01;
const CHECK_DATA: u8 = 0x02;
const CHECK_EXTRA: u8 = 0x04;
const END_OF_LIST: u16 = 0xffff;
const CRIT_RETURN: u8 = 0xc3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FarPtr {
    pub segment: u16,
    pub offset: u16,
}

pub struct Segments {
    pub stack: u16,
    pub code: u16,
    pub data: u16,
    pub extra: u16,
}

#[derive(Clone, Copy)]
pub struct SftSet<'a> {
    pub open: &'a [FarPtr],
    pub fcb: &'a [FarPtr],
    pub aux: FarPtr,
    pub rename_buf: FarPtr,
}

/// `SegCheck`. Bit 0 selects CS. Bit 1 checks DS. Bit 2 checks ES.
pub fn seg_check(flags: u8, segments: Segments) -> (bool, bool) {
    let baseline = if flags & USE_CODE != 0 {
        segments.code
    } else {
        segments.stack
    };
    let data_bad = flags & CHECK_DATA != 0 && segments.data != baseline;
    let extra_bad = flags & CHECK_EXTRA != 0 && segments.extra != baseline;
    (data_bad, extra_bad)
}

/// `DPBCheck`. The first node whose offset is `FFFFh` ends the walk.
pub fn dpb_found(chain: &[FarPtr], wanted: FarPtr) -> bool {
    list_found(chain, wanted)
}

/// `SFTCheck`. Open handles, then the FCB table, then the two static slots.
pub fn sft_found(tables: SftSet<'_>, wanted: FarPtr) -> bool {
    if list_found(tables.open, wanted) {
        return true;
    }
    if list_found(tables.fcb, wanted) {
        return true;
    }
    if same(tables.aux, wanted) {
        return true;
    }
    same(tables.rename_buf, wanted)
}

/// `BUFCheck` critical-section preamble. A `RET` entry skips both tests.
pub fn disk_crit(entry: u8, owner: u16, current: u16, refs: u16) -> (bool, bool) {
    if entry == CRIT_RETURN {
        return (false, false);
    }
    let other_owner = owner != current;
    let zero_refs = refs == 0;
    (other_owner, zero_refs)
}

/// `BUFCheck` list walk.
pub fn buf_found(chain: &[FarPtr], wanted: FarPtr) -> bool {
    list_found(chain, wanted)
}

pub fn show() {
    let data_only = seg_check(
        CHECK_DATA,
        Segments {
            stack: 0x1000,
            code: 0x2000,
            data: 0x3000,
            extra: 0x3000,
        },
    );
    let against_code = seg_check(
        USE_CODE | CHECK_DATA,
        Segments {
            stack: 0x1000,
            code: 0x2000,
            data: 0x2000,
            extra: 0x1000,
        },
    );
    let extra_only = seg_check(
        CHECK_EXTRA,
        Segments {
            stack: 0x1000,
            code: 0x2000,
            data: 0x1000,
            extra: 0x3000,
        },
    );
    let end = FarPtr {
        segment: 0,
        offset: END_OF_LIST,
    };
    let second = FarPtr {
        segment: 0x70,
        offset: 0x20,
    };
    let dpb_end = dpb_found(&[end, second], second);
    let dpb_hit = dpb_found(
        &[
            FarPtr {
                segment: 0x70,
                offset: 0x10,
            },
            second,
            end,
        ],
        second,
    );
    let aux = FarPtr {
        segment: 0x80,
        offset: 0x40,
    };
    let rename_buf = FarPtr {
        segment: 0x80,
        offset: 0x50,
    };
    let tables = SftSet {
        open: &[FarPtr {
            segment: 0x80,
            offset: 0x10,
        }],
        fcb: &[FarPtr {
            segment: 0x80,
            offset: 0x30,
        }],
        aux,
        rename_buf,
    };
    let sft_miss = sft_found(tables, FarPtr {
        segment: 0x80,
        offset: 0x99,
    });
    let sft_hit = sft_found(tables, rename_buf);
    let (nop_owner, _) = disk_crit(CRIT_RETURN, 1, 2, 0);
    let (other_owner, zero_refs) = disk_crit(0, 1, 2, 0);
    let buf_hit = buf_found(
        &[
            FarPtr {
                segment: 0x60,
                offset: 0x10,
            },
            end,
        ],
        FarPtr {
            segment: 0x60,
            offset: 0x10,
        },
    );
    let buf_end = buf_found(
        &[
            end,
            FarPtr {
                segment: 0x60,
                offset: 0x10,
            },
        ],
        FarPtr {
            segment: 0x60,
            offset: 0x10,
        },
    );
    crate::bios::mscon::con_write_bytes(b"SEGCHECK DS");
    put_flag(data_only.0);
    crate::bios::mscon::con_write_bytes(b" CS");
    put_flag(against_code.0);
    crate::bios::mscon::con_write_bytes(b" ES");
    put_flag(extra_only.1);
    crate::bios::mscon::con_write_bytes(b" DP");
    put_flag(dpb_end);
    crate::bios::mscon::con_write_bytes(b" DP");
    put_flag(dpb_hit);
    crate::bios::mscon::con_write_bytes(b" SF");
    put_flag(sft_miss);
    crate::bios::mscon::con_write_bytes(b" SF");
    put_flag(sft_hit);
    crate::bios::mscon::con_write_bytes(b" NP");
    put_flag(nop_owner);
    crate::bios::mscon::con_write_bytes(b" OW");
    put_flag(other_owner);
    crate::bios::mscon::con_write_bytes(b" ZR");
    put_flag(zero_refs);
    crate::bios::mscon::con_write_bytes(b" BU");
    put_flag(buf_hit);
    crate::bios::mscon::con_write_bytes(b" BE");
    put_flag(buf_end);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn list_found(chain: &[FarPtr], wanted: FarPtr) -> bool {
    for node in chain {
        if node.offset == END_OF_LIST {
            return false;
        }
        if same(*node, wanted) {
            return true;
        }
    }
    false
}

fn same(left: FarPtr, right: FarPtr) -> bool {
    left.segment == right.segment && left.offset == right.offset
}

fn put_flag(set: bool) {
    crate::bios::mscon::con_write_bytes(if set { b"01" } else { b"00" });
}
