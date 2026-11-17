unsafe extern "Rust" {
    fn dos_memory_call(regs: *mut u16);
}

pub fn call(a: u16, b: u16, c: u16, d: u16) -> [u16; 12] {
    let mut r = [0; 12];
    r[0] = a;
    r[1] = b;
    r[2] = c;
    r[3] = d;
    r[9] = 3;
    unsafe { dos_memory_call(r.as_mut_ptr()) };
    r
}

pub fn write(handle: u16, data: &[u8]) {
    call(0x4000, handle, data.len() as u16, data.as_ptr() as u16);
}

pub fn hex(out: &mut [u8], at: usize, word: u16) {
    let digits = b"0123456789ABCDEF";
    let mut i = 0;
    while i < 4 {
        out[at + i] = digits[((word >> (12 - i * 4)) & 15) as usize];
        i += 1;
    }
}
