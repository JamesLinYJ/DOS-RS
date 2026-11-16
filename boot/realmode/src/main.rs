#![no_std]
#![no_main]
use dosrs_realmode as bios;

#[unsafe(no_mangle)]
pub fn main16() {
    let cpu = bios::cpu();
    if cpu < 286 {
        bios::print("DOS requires an 80286 or newer.\r\n");
        return;
    }
    bios::command(if cpu == 286 { "K286.SYS" } else { "K386.SYS" });
    let res = bios::reserved();
    if res != 0
        && bios::filename(bios::COMMAND, 0, 8)
        && bios::load(res, bios::KERNEL, 0xf000).is_some()
    {
        bios::start_kernel();
    }
    bios::print("DOS kernel read failed.\r\n");
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    bios::halt()
}
