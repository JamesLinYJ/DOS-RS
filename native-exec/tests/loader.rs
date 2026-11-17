// Use real linker output to exercise both ELF header layouts and segment permissions.
use dosrs_native_exec::{ANON_BASE, BASE, END, Frame, Machine, PAGE, STACK};
use std::{fs, process::Command};

#[repr(align(4096))]
struct Page([u8; PAGE]);

thread_local! {
    static REMAINING: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}
struct TestFrame(Box<Page>);
impl Drop for TestFrame {
    fn drop(&mut self) {
        REMAINING.with(|left| left.set(left.get() + 1));
    }
}
impl Frame for TestFrame {
    fn allocate_zeroed() -> dosrs_native_exec::Result<Self> {
        REMAINING.with(|left| {
            let n = left
                .get()
                .checked_sub(1)
                .ok_or("insufficient physical memory")?;
            left.set(n);
            Ok(Self(Box::new(Page([0; PAGE]))))
        })
    }
    // Simulate a stale availability snapshot, as another CPU can claim pages.
    fn available() -> usize {
        usize::MAX
    }
    fn frame_address(&self) -> usize {
        self.0.as_ref() as *const Page as usize
    }
    fn read(&self, offset: usize, out: &mut [u8]) {
        out.copy_from_slice(&self.0.0[offset..offset + out.len()]);
    }
    fn write(&mut self, offset: usize, input: &[u8]) {
        self.0.0[offset..offset + input.len()].copy_from_slice(input);
    }
}
type Image = dosrs_native_exec::Image<TestFrame>;

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

fn exercise(elf: &[u8], machine: Machine, other: Machine) {
    assert!(Image::load(elf, other).is_err());
    let mut image = Image::load(elf, machine).unwrap();
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
    REMAINING.with(|left| {
        let saved = left.replace(2);
        assert_eq!(image.map_anonymous(3), Err("insufficient physical memory"));
        assert_eq!(left.replace(saved), 2);
        assert!(image.check(ANON_BASE, 1, false).is_err());
    });
    let base = image.map_anonymous(600).unwrap();
    assert_eq!(base, ANON_BASE);
    image.write(base + 575 * PAGE, &[44]).unwrap();
    image.unmap_anonymous(base + 575 * PAGE, 1).unwrap();
    assert!(image.unmap_anonymous(base + 574 * PAGE, 2).is_err());
    assert_eq!(image.map_anonymous(1).unwrap(), base + 575 * PAGE);
    image.read(base + 575 * PAGE, &mut last).unwrap();
    assert_eq!(last, [0]);
    image.unmap_anonymous(base, 600).unwrap();
    assert!(image.check(base, 1, false).is_err());
    assert!(Image::load(&elf[..50], machine).is_err());
}
#[test]
fn i686_static_c_image() {
    exercise(&build("i686"), Machine::I386, Machine::Amd64);
}
#[test]
fn x86_64_static_c_image() {
    let elf = build("x86_64");
    exercise(&elf, Machine::Amd64, Machine::I386);

    let phoff = u64::from_le_bytes(elf[32..40].try_into().unwrap()) as usize;
    let phsize = u16::from_le_bytes(elf[54..56].try_into().unwrap()) as usize;
    let relocated = |text: u64, data: u64| {
        let mut file = elf.clone();
        file[24..32].copy_from_slice(&text.to_le_bytes()); // e_entry
        for (index, addr) in [text, data].into_iter().enumerate() {
            let start = phoff + index * phsize + 16; // p_vaddr
            file[start..start + 8].copy_from_slice(&addr.to_le_bytes());
        }
        Image::load(&file, Machine::Amd64)
    };
    let top = dosrs_native_exec::USER_END;
    let code = top - (1 << 40);
    let data = top - 3 * PAGE as u64;
    let mut image = relocated(code, data).unwrap();
    assert_eq!(image.entry as u64, code);
    assert!(image.pages().map(|(addr, _)| addr).is_sorted());
    assert!(image.write(code as usize, &[1]).is_err());
    image.write(data as usize + PAGE - 1, &[9, 8]).unwrap();
    let mut bytes = [0; 2];
    image.read(data as usize + PAGE - 1, &mut bytes).unwrap();
    assert_eq!(bytes, [9, 8]);
    assert!(image.check(top as usize - 1, 2, false).is_err());
    for bad in [
        0x8000_0000,
        top - PAGE as u64,
        top,
        0xffff_8000_0000_0000,
        u64::MAX - 4095,
    ] {
        assert!(
            relocated(BASE as u64, bad).is_err(),
            "mapped invalid segment at {bad:#x}"
        );
    }
}
