//! Observe LOADHIGH placement, console handles, and text-mode output via DOS process APIs.
#![no_std]
#![no_main]
use dos_tools as _;
use dosrs_realmode::{byte, system::w};
#[path = "support/command_probe.rs"]
mod probe;
use probe::{call, hex, write};

#[unsafe(no_mangle)]
pub fn main16() -> u16 {
    let psp = call(0x6200, 0, 0, 0)[1];
    let op = byte(psp, 0x82) as u8;
    if op == b'T' {
        let file = call(0x3d00, 0, 0, b"C:\\KEY.IN\0".as_ptr() as u16)[0];
        let device = call(0x3d02, 0, 0, b"NUL\0".as_ptr() as u16)[0];
        let f = call(0x4401, file, 0, 0x83);
        let d = call(0x4401, device, 0, 0x180);
        let original = call(0x4400, device, 0, 0)[3];
        let set = call(0x4401, device, 0, 0x20);
        let info = call(0x4400, device, 0, 0);
        let mut line = *b"0000 0000 0000 0000 0000 0000 0000\r\n";
        hex(&mut line, 0, f[10] & 1);
        hex(&mut line, 5, f[0]);
        hex(&mut line, 10, d[10] & 1);
        hex(&mut line, 15, d[0]);
        hex(&mut line, 20, set[10] & 1);
        hex(&mut line, 25, set[0]);
        hex(&mut line, 30, info[3]);
        call(0x4401, device, 0, original & 255);
        write(1, &line);
        call(0x3e00, file, 0, 0);
        call(0x3e00, device, 0, 0);
    } else if op == b'M' {
        let previous = call(0x5802, 0, 0, 0)[0] & 255;
        call(0x5803, 1, 0, 0);
        let list = call(0x5200, 0, 0, 0);
        let mut m = w(list[6], list[1] - 2);
        loop {
            let mut line = *b"0000 0000 0000 ........\r\n";
            let size = w(m, 3);
            hex(&mut line, 0, m);
            hex(&mut line, 5, w(m, 1));
            hex(&mut line, 10, size);
            for i in 0..8 {
                line[15 + i] = byte(m, 8 + i as u16) as u8;
            }
            write(1, &line);
            if byte(m, 0) != 77 || size >= u16::MAX - m {
                break;
            }
            m += size + 1;
        }
        call(0x5803, previous, 0, 0);
    } else if op == b'V' || op == b'I' {
        let mut name = *b"C:\\STATE0.OUT\0";
        name[8] = byte(psp, 0x83) as u8;
        let fd = call(0x3c00, 0, 0, name.as_ptr() as u16)[0];
        if op == b'V' {
            let video = if byte(0x40, 0x49) == 7 {
                0xb000
            } else {
                0xb800
            };
            let base = w(0x40, 0x4e);
            let mut row = [0; 80];
            let mut i = 0;
            while i < 25 {
                let mut j = 0;
                while j < row.len() {
                    row[j] = byte(video, base + (i * 80 + j as u16) * 2) as u8;
                    j += 1;
                }
                write(fd, &row);
                i += 1;
            }
        } else {
            let mut line = *b"0000 0000 0000\r\n";
            let mut fd_num = 0;
            while fd_num < 3 {
                hex(
                    &mut line,
                    usize::from(fd_num) * 5,
                    call(0x4400, fd_num, 0, 0)[3],
                );
                fd_num += 1;
            }
            write(fd, &line);
        }
        call(0x3e00, fd, 0, 0);
        if op == b'V' {
            call(0x0700, 0, 0, 0);
        }
    } else if op == b'G' {
        // Select the native text console for direct BDA/VRAM assertions. Standard DOS starts in text mode.
        if matches!(dos_tools::screen(7, 0, 0), 6 | 7 | 16 | 18) {
            dos_tools::screen(6, 7, 0);
        }
    }
    0
}
