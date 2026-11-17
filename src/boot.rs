//! Rust UEFI OS loader. Firmware protocols are closed before kernel entry.
mod legacy;
mod secondary;
use crate::platform::MAX_CPUS;
use uefi::boot::{AllocateType, MemoryAttribute, MemoryType};
use uefi::mem::memory_map::MemoryMap;
use uefi::proto::{
    console::gop::{GraphicsOutput, PixelFormat},
    pi::mp::MpServices,
};
use uefi::{Status, boot, entry};

pub(crate) struct Handoff {
    pub ids: [u8; MAX_CPUS],
    pub count: usize,
    pub available: usize,
    pub vector: Option<u8>,
}
fn processors() -> Handoff {
    let mut out = Handoff {
        ids: [0; MAX_CPUS],
        count: 1,
        available: 1,
        vector: None,
    };
    out.ids[0] = crate::arch::platform::boot_cpu().0;
    let Ok(handle) = boot::get_handle_for_protocol::<MpServices>() else {
        return out;
    };
    let Ok(mp) = boot::open_protocol_exclusive::<MpServices>(handle) else {
        return out;
    };
    let Ok(cpus) = mp.get_number_of_processors() else {
        return out;
    };
    out.available = cpus.enabled;
    for n in 0..cpus.total {
        let Ok(cpu) = mp.get_processor_info(n) else {
            continue;
        };
        if cpu.is_enabled()
            && cpu.is_healthy()
            && !cpu.is_bsp()
            && let Ok(id) = u8::try_from(cpu.processor_id)
            && out.count < MAX_CPUS
            && !out.ids[..out.count].contains(&id)
        {
            out.ids[out.count] = id;
            out.count += 1;
        }
    }
    out
}
fn video() -> Option<(u64, u64)> {
    let Ok(h) = boot::get_handle_for_protocol::<GraphicsOutput>() else {
        return None;
    };
    let Ok(mut gop) = boot::open_protocol_exclusive::<GraphicsOutput>(h) else {
        return None;
    };
    // 640×480 gives DOS text its native 8×16 pixel cell on a PC display.
    let mode = gop.modes().find(|m| {
        let info = m.info();
        info.resolution() == (640, 480)
            && matches!(info.pixel_format(), PixelFormat::Rgb | PixelFormat::Bgr)
    });
    if let Some(mode) = mode {
        let _ = gop.set_mode(&mode);
    }
    let info = gop.current_mode_info();
    if !matches!(info.pixel_format(), PixelFormat::Rgb | PixelFormat::Bgr) {
        return None;
    }
    let (w, h) = info.resolution();
    let mut fb = gop.frame_buffer();
    crate::bios::screen::initialize(
        fb.as_mut_ptr() as usize,
        fb.size(),
        w,
        h,
        info.stride(),
        info.pixel_format() == PixelFormat::Rgb,
    );
    Some((fb.as_mut_ptr() as usize as u64, fb.size() as u64))
}
#[entry]
fn main() -> Status {
    let _ = boot::set_watchdog_timer(0, 0x10000, None);
    let mut handoff = processors();
    let framebuffer = video();
    if handoff.count > 1 {
        let Ok(low) = boot::allocate_pages(
            AllocateType::MaxAddress(0xfffff),
            MemoryType::LOADER_CODE,
            1,
        ) else {
            return Status::OUT_OF_RESOURCES;
        };
        if let Some(vector) = secondary::prepare(low) {
            handoff.vector = Some(vector);
        } else {
            return Status::UNSUPPORTED;
        }
    }
    // All protocol guards have been dropped. Kernel stacks, code, handoff pages,
    // framebuffer and the current paging structures are retained for kernel lifetime.
    let map = unsafe { boot::exit_boot_services(Some(MemoryType::LOADER_DATA)) };
    // Loader/firmware allocations, runtime pages, ACPI, MMIO and the memory
    // map itself remain reserved. Donate only ordinary conventional RAM.
    let regions = map.entries().map(|d| {
        let usable = d.ty == MemoryType::CONVENTIONAL
            && !d.att.intersects(
                MemoryAttribute::RUNTIME
                    | MemoryAttribute::NON_VOLATILE
                    | MemoryAttribute::SPECIAL_PURPOSE,
            );
        (d.phys_start, d.page_count.saturating_mul(4096), usable)
    });
    crate::arch::frames::initialize(regions, framebuffer.as_slice());
    core::mem::forget(map);
    unsafe {
        x86::irq::disable();
    }
    crate::arch::outb(0x21, 0xff);
    crate::arch::outb(0xa1, 0xff);
    crate::arch::io_memory::firmware_mmio();
    crate::kmain(handoff, "UEFI boot services exited.")
}
