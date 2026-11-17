//! Boot-only probe of a live CR3 retirement on a second CPU.
use super::{AddressSpace, Thread};
use crate::{
    arch::frames::UserImage,
    tasks::user::{self, Completion},
    vfs::dos,
};
use alloc::vec;
use core::task::Waker;
use dosrs_native_exec::{ANON_BASE, END, Machine};

pub(crate) fn run() -> Result<(), &'static str> {
    if crate::platform::cpu_count() < 2 {
        return Err("TLB test needs two online CPUs");
    }
    dos::boot();
    let dir = dos::drive_root(2).ok_or("TLB test needs a C drive")?;
    let file = dos::OpenFile::open(dir, b"TLB     ELF", false).map_err(|_| "missing TLB.ELF")?;
    let mut bytes = vec![0u8; file.len()];
    let mut at = 0;
    while at < bytes.len() {
        let n = file
            .read(at, &mut bytes[at..])
            .map_err(|_| "ELF read failed")?;
        if n == 0 {
            return Err("truncated TLB.ELF");
        }
        at += n;
    }
    let machine = if cfg!(target_arch = "x86") {
        Machine::I386
    } else {
        Machine::Amd64
    };
    let mut space = AddressSpace::from(UserImage::load(&bytes, machine)?);
    let page = space.map_anonymous(1)?;
    if page != ANON_BASE {
        return Err("unexpected handshake address");
    }

    // A timer can legitimately win the race with the IPI. Only accept a
    // real ring-3 IPI on the AP, and bound retries to diagnose a broken path.
    for _ in 0..8 {
        space.prepare()?;
        space.write(page, &[0])?;
        let root = space.page_table();
        let thread = Thread::try_new(space.entry, END - 32, 0, 0)?;
        let result = Completion::new()?;
        user::submit(1, thread, root.clone(), &result, Waker::noop())
            .map_err(|_| "AP work submission failed")?;
        let mut flag = [0];
        while flag[0] == 0 && !result.ready() {
            space.read(page, &mut flag)?;
            core::hint::spin_loop();
        }
        space.invalidate();
        while !result.ready() {
            core::hint::spin_loop();
        }
        let returned = result.take().ok_or("AP did not return")?;
        let _ = returned.outcome?;
        if flag != [0x5a] || returned.thread.regs.vector != 0xf1 {
            continue;
        }
        if returned.thread.regs.cs & 3 != 3 || root.acquire(0).is_ok() {
            return Err("old user root is still executable");
        }
        space.unmap_anonymous(page, 1)?;
        if space.read(page, &mut flag).is_ok() {
            return Err("retired frame remains mapped");
        }
        crate::puts(
            "TLB: AP1 ring3 handshake, vector F1, host CR3 restored, stale root rejected, page unmapped\n",
        );
        return Ok(());
    }
    Err("no live user F1 acknowledgement")
}
