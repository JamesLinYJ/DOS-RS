// Use real linker output to exercise both ELF header layouts and segment permissions.
use dosrs_native_exec::{BASE, END, Image, Machine, PAGE, STACK};
use std::{fs, process::Command};

fn build(target: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("dosrs-elf-{}-{target}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("probe.c"), "volatile unsigned char bss[9000]; unsigned char data[]={1,2,3}; void entry(void) { bss[1] = data[1]; }").unwrap();
    fs::write(dir.join("user.ld"), format!("ENTRY(entry)\nPHDRS {{ code PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }}\nSECTIONS {{ . = {BASE}; .text : {{ *(.text*) }} :code . = ALIGN(4096); .data : {{ *(.data*) *(.bss*) }} :data /DISCARD/ : {{ *(.eh_frame*) *(.comment*) *(.note*) }} }}")).unwrap();
    let status = Command::new(format!("{target}-elf-gcc"))
        .current_dir(&dir)
        .args([
            "-ffreestanding",
            "-fno-pie",
            "-nostdlib",
            "-Wl,-T,user.ld",
            "-Wl,-z,max-page-size=4096",
            "probe.c",
            "-o",
            "probe.elf",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let file = fs::read(dir.join("probe.elf")).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    file
}

fn exercise(target: &str, machine: Machine, other: Machine) {
    let elf = build(target);
    assert!(Image::load(&elf, other).is_err());
    let mut image = Image::load(&elf, machine).unwrap();
    assert_eq!(image.entry, BASE);
    assert!(image.write(BASE, &[0]).is_err());
    let mut data = [0; 3];
    image.read(BASE + PAGE, &mut data).unwrap();
    assert_eq!(data, [1, 2, 3]);
    image.read(BASE + 2 * PAGE, &mut data).unwrap();
    assert_eq!(data, [0; 3]);
    image.write(STACK + PAGE - 1, &[1, 2, 3]).unwrap();
    image.read(STACK + PAGE - 1, &mut data).unwrap();
    assert_eq!(data, [1, 2, 3]);
    assert!(image.write(STACK - 1, &[0, 0]).is_err());
    assert!(image.read(usize::MAX - 1, &mut data).is_err());
    assert!(image.write(END - 1, &[1, 2]).is_err());
    let mut last = [1];
    image.read(END - 1, &mut last).unwrap();
    assert_eq!(last, [0]);
    assert!(Image::load(&elf[..50], machine).is_err());
}
#[test]
fn i686_static_c_image() {
    exercise("i686", Machine::I386, Machine::Amd64);
}
#[test]
fn x86_64_static_c_image() {
    exercise("x86_64", Machine::Amd64, Machine::I386);
}
