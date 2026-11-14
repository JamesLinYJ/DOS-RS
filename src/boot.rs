//! Rust UEFI OS loader. Firmware protocols are closed before kernel entry.
mod legacy;
mod secondary;
use crate::platform::MAX_CPUS;
use uefi::boot::{AllocateType, MemoryType};
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
fn video() {
    let Ok(h) = boot::get_handle_for_protocol::<GraphicsOutput>() else {
        return;
    };
    let Ok(mut gop) = boot::open_protocol_exclusive::<GraphicsOutput>(h) else {
        return;
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
        return;
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
}
#[entry]
fn main() -> Status {
    let _ = boot::set_watchdog_timer(0, 0x10000, None);
    let mut handoff = processors();
    video();
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
    core::mem::forget(map);
    unsafe {
        x86::irq::disable();
    }
    crate::arch::outb(0x21, 0xff);
    crate::arch::outb(0xa1, 0xff);
    crate::arch::io_memory::firmware_mmio();
    crate::kmain(handoff, "UEFI boot services exited.")
}
