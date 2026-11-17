//! Native DOS bootstrap. COMMAND.COM owns the prompt, batch files and command dispatch.
#[path = "realmode_arena.rs"]
mod arena;
#[path = "realmode_env.rs"]
mod environment;
#[path = "realmode_files.rs"]
mod files;
#[path = "realmode_mz.rs"]
mod mz;
use dosrs_realmode as pc;

#[unsafe(no_mangle)]
pub fn main16() {
    mz::boot_memory();
    pc::dos_tables::init();
    unsafe extern "Rust" {
        fn dos16_bind_mux();
    }
    unsafe { dos16_bind_mux() };
    arena::init();
    let boot = if pc::boot_unit() >= 128 {
        2
    } else {
        pc::boot_unit()
    };
    pc::store(pc::COMMAND, 256, boot);
    let mut volume = pc::disk::Volume::EMPTY;
    pc::cdrom::probe();
    if volume.mount() {
        pc::print(if volume.is32() {
            "C: FAT32\r\n"
        } else {
            "C: FAT16\r\n"
        });
    }
    #[cfg(rust16_cpu = "286")]
    pc::print("DOS / 80286\r\n");
    #[cfg(rust16_cpu = "386")]
    pc::print("DOS / 80386\r\n");
    loop {
        files::begin();
        if !mz::interpreter(&mut volume, boot) {
            pc::print("Cannot load the command interpreter\r\n");
            return;
        }
        files::finish();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    pc::halt()
}

#[unsafe(no_mangle)]
pub fn dos16_mux(ss: u16, sp: u16) -> u16 {
    pc::control(31, ss, sp, 0)
}
