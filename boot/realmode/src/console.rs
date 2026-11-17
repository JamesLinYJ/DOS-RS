//! Resident console. Both bytes of a double-width glyph share one cursor operation.
use dosrs_realmode::{
    byte, store,
    system::{dw, sd, sw, w},
    video,
};
const STATE: u16 = 0x37b0;
use crate::font::{self, rb, rd};

unsafe extern "Rust" {
    fn video_detect() -> u16;
    fn serial_char(c: u16);
    fn bios_scan() -> u16;
    fn write_port(port: u16, val: u16);
}
fn port(p: u16, v: u16) {
    unsafe { write_port(p, v) }
}
fn field(at: u16) -> u16 {
    w(STATE, at)
}
fn set(at: u16, x: u16) {
    sw(STATE, at, x)
}
pub fn init(unit: u16, pages: u16) {
    set(0, unit);
    set(2, pages);
    set(4, 0xffff);
    let card = unsafe { video_detect() };
    let mode = if card == 7 || card == 8 || card == 11 || card == 12 {
        18
    } else if card == 4 || card == 5 {
        16
    } else if byte(0x40, 0x49) == 7 {
        7
    } else {
        6
    };
    if !font::init(unit, pages) {
        set(2, 0)
    }
    mode_set(mode);
}
pub fn mode_set(mode: u16) {
    if !matches!(mode, 6 | 7 | 16 | 18) {
        return;
    }
    let bios_mode = if mode == 7 && byte(0x40, 0x10) & 0x30 != 0x30 {
        3
    } else {
        mode
    };
    video(bios_mode, 0, 0, 0);
    set(6, mode);
    set(8, 0);
    set(10, 0);
    set(12, 0);
    set(14, 15);
    set(16, 0);
    set(24, 1);
    set(26, 0);
}
fn glyph(code: u32) -> u32 {
    if code < 128 {
        return 32 + (code << 5);
    }
    if code <= 65535 {
        let a = (code >> 8) as u16;
        let b = (code & 255) as u16;
        if a >= 129 && a <= 254 && b >= 64 && b != 127 && b <= 254 {
            return 4128
                + (((a - 129) as u32 * 190 + (b - 64 - if b > 127 { 1 } else { 0 }) as u32) << 5);
        }
    } else {
        if let Some(bitmap) = font::four_byte(code) {
            return bitmap;
        }
        let mut lo = 0;
        let mut hi = rd(4);
        let index = rd(8);
        let data = rd(12);
        if hi > 1000000 {
            return 0;
        }
        while lo < hi {
            let m = lo + ((hi - lo) >> 1);
            let k = rd(index + (m << 2));
            if k < code {
                lo = m + 1
            } else if k > code {
                hi = m
            } else {
                return data + (m << 5);
            }
        }
    }
    0
}
fn rows() -> u16 {
    match field(6) {
        6 => 12,
        16 => 21,
        18 => 30,
        _ => 25,
    }
}
fn planar(mask: u16) {
    port(0x3c4, 2);
    port(0x3c5, mask);
}
fn addr(mode: u16, y: u16, x: u16) -> u16 {
    if mode == 6 {
        (y & 1) * 8192 + (y >> 1) * 80 + x
    } else {
        y * 80 + x
    }
}
fn monochrome(color: u16) -> bool {
    matches!(color, 7 | 10 | 11 | 14 | 15)
}
fn solid(mode: u16, at: u16, color: u16) {
    if mode == 6 {
        store(0xb800, at, if monochrome(color) { 255 } else { 0 })
    } else {
        planar(15);
        store(0xa000, at, 0);
        if color != 0 {
            planar(color);
            store(0xa000, at, 255)
        }
    }
}
fn paint(mode: u16, at: u16, bits: u16, fg: u16, bg: u16) {
    if mode == 6 {
        store(
            0xb800,
            at,
            if monochrome(fg) == monochrome(bg) {
                if monochrome(bg) { 255 } else { 0 }
            } else if monochrome(fg) {
                bits
            } else {
                bits ^ 255
            },
        );
        return;
    }
    planar(15);
    store(0xa000, at, 0);
    if fg & !bg != 0 {
        planar(fg & !bg);
        store(0xa000, at, bits);
    }
    if bg & !fg != 0 {
        planar(bg & !fg);
        store(0xa000, at, bits ^ 255);
    }
    if fg & bg != 0 {
        planar(fg & bg);
        store(0xa000, at, 255);
    }
}
fn draw(start: u32, width: u16) {
    let mode = field(6);
    let scale = if field(24) == 2 { 2 } else { 1 };
    let x = field(8);
    let y = field(10) * 16;
    let fg = field(14);
    let bg = field(16);
    let mut row = 0;
    while row < 16 {
        let mut col = 0;
        while col < width {
            let bits = if start == 0 {
                if row == 0 || row == 15 {
                    255
                } else if col == 0 {
                    128
                } else {
                    1
                }
            } else {
                rb(start + row as u32 * 2 + col as u32)
            };
            if scale == 1 {
                paint(mode, addr(mode, y + row, x + col), bits, fg, bg)
            } else {
                let mut expanded = 0;
                let mut n = 0;
                while n < 8 {
                    expanded = (expanded << 2) | if bits & (128 >> n) != 0 { 3 } else { 0 };
                    n += 1;
                }
                let mut k = 0;
                while k < 2 && y + row * 2 + k < rows() * 16 {
                    let at = addr(mode, y + row * 2 + k, x + col * 2);
                    paint(mode, at, expanded >> 8, fg, bg);
                    paint(mode, at + 1, expanded & 255, fg, bg);
                    k += 1;
                }
            }
            col += 1;
        }
        row += 1;
    }
    if mode != 6 {
        planar(15)
    }
}
fn rectangle(origin: u16, size: u16) {
    let x = origin & 255;
    let y = origin >> 8;
    let width = size & 255;
    let height = size >> 8;
    let mode = field(6);
    let bg = field(16);
    if width == 0 || height == 0 || x + width > 80 || y + height > rows() {
        return;
    }
    if mode == 7 {
        let attr = if monochrome(bg) { 0x70 } else { 7 };
        video(
            0x0600,
            attr << 8,
            (y << 8) | x,
            ((y + height - 1) << 8) | (x + width - 1),
        );
        return;
    }
    // Fill a full scanline at a time, selecting planes only once per pass.
    let mut pass = 0;
    while pass < 2 {
        if pass == 1 && (mode == 6 || bg == 0) {
            break;
        }
        if mode != 6 {
            planar(if pass == 0 { 15 } else { bg })
        }
        let value = if pass == 1 || (mode == 6 && monochrome(bg)) {
            255
        } else {
            0
        };
        let mut scan = y * 16;
        while scan < (y + height) * 16 {
            let at = addr(mode, scan, x);
            let mut j = 0;
            while j < width {
                store(if mode == 6 { 0xb800 } else { 0xa000 }, at + j, value);
                j += 1;
            }
            scan += 1;
        }
        pass += 1;
    }
    if mode != 6 {
        planar(15)
    }
}
fn theme() {
    set(26, 1);
    if field(6) != 18 {
        return;
    }
    // Logical palette indices are shared with EGA; the VGA DAC supplies the installation palette.
    // Components are stored in the DAC's six-bit format.
    const RGB: &[u8; 48] = &[
        0x06, 0x0a, 0x0b, 0x06, 0x0c, 0x0e, 0x02, 0x19, 0x14, 0x02, 0x1d, 0x19, 0x27, 0x0e, 0x0b,
        0x19, 0x12, 0x1a, 0x25, 0x1a, 0x06, 0x39, 0x3a, 0x38, 0x12, 0x17, 0x18, 0x22, 0x26, 0x24,
        0x2f, 0x39, 0x34, 0x19, 0x36, 0x2f, 0x3a, 0x2d, 0x28, 0x31, 0x2f, 0x2b, 0x3c, 0x36, 0x23,
        0x3e, 0x3e, 0x3d,
    ];
    let mut i = 0;
    while i < 16 {
        video(0x1000, (i << 8) | i, 0, 0);
        let p = i as usize * 3;
        let r = u16::from(RGB[p]);
        let g = u16::from(RGB[p + 1]);
        let b = u16::from(RGB[p + 2]);
        video(0x1010, i, (g << 8) | b, r << 8);
        i += 1;
    }
}
fn newline() {
    let mut y = field(10) + 1;
    if y >= rows() {
        // BIOS scroll uses character heights of 8, 14 or 16. Scroll whole
        // software rows directly in CGA; VGA/EGA use graphics read/write latches.
        let mode = field(6);
        let end = rows() * 16;
        if mode != 6 {
            port(0x3ce, 5);
            port(0x3cf, 1);
            planar(15);
        }
        let seg = if mode == 6 { 0xb800 } else { 0xa000 };
        let mut scan = 16;
        while scan < end {
            dosrs_realmode::copy_bytes(seg, addr(mode, scan, 0), seg, addr(mode, scan - 16, 0), 80);
            scan += 1;
        }
        if mode != 6 {
            port(0x3ce, 5);
            port(0x3cf, 0);
        }
        scan = end - 16;
        while scan < end {
            let mut x = 0;
            while x < 80 {
                store(seg, addr(mode, scan, x), 0);
                x += 1;
            }
            scan += 1;
        }
        y = rows() - 1;
    }
    set(10, y);
}
fn emit(code: u32, mut width: u16) {
    let start = glyph(code);
    if code > 127 && code != u32::MAX {
        if start != 0 {
            let mut i = 0;
            while i < 16 && rb(start + i * 2 + 1) == 0 {
                i += 1;
            }
            if i == 16 {
                width = 1
            }
        }
    }
    let advance = width * if field(24) == 2 { 2 } else { 1 };
    if field(8) + advance > 80 {
        set(8, 0);
        newline()
    }
    draw(start, width);
    let x = field(8) + advance;
    if x == 80 {
        set(8, 0);
        newline()
    } else {
        set(8, x)
    }
}
pub fn output(c: u16) {
    unsafe { serial_char(c) }
    // A DOS application can change the adapter directly through INT 10h.
    // Keep its active BIOS page and CP437 characters visible in text mode.
    let mode = byte(0x40, 0x49) & 0x7f;
    if field(6) != 7 && mode != field(6) {
        let page = byte(0x40, 0x62);
        video(0x0e00 | c, (page << 8) | 7, 0, 0);
        return;
    }
    if field(6) == 7 && field(26) == 1 && c >= 32 {
        let x = field(8);
        let y = field(10);
        let attr = if monochrome(field(16)) { 0x70 } else { 7 };
        video(0x0200, 0, 0, (y << 8) | x);
        video(0x0900 | if c < 128 { c } else { 63 }, attr, 1, 0);
        set(8, if x < 79 { x + 1 } else { 0 });
        return;
    }
    if field(6) == 7 || field(2) == 0 {
        video(0x0e00 | (if c < 128 { c } else { 63 }), 7, 0, 0);
        return;
    }
    let mut count = field(12);
    if count != 0 {
        let val = dw(STATE, 20);
        if (count == 1 && c >= 48 && c <= 57) || (count == 2 && c >= 129 && c <= 254) {
            sd(STATE, 20, (val << 8) | c as u32);
            set(12, count + 1);
            return;
        }
        if (count == 1 && c >= 64 && c != 127 && c <= 254) || (count == 3 && c >= 48 && c <= 57) {
            set(12, 0);
            emit((val << 8) | c as u32, 2);
            return;
        }
        set(12, 0);
        emit(u32::MAX, 2);
        count = 0;
    }
    if count == 0 && c >= 129 && c <= 254 {
        sd(STATE, 20, c as u32);
        set(12, 1);
        return;
    }
    match c {
        13 => set(8, 0),
        10 => newline(),
        8 => {
            if field(8) > 0 {
                set(8, field(8) - 1)
            }
        }
        9 => {
            let n = 8 - (field(8) & 7);
            let mut i = 0;
            while i < n {
                emit(32, 1);
                i += 1;
            }
        }
        32..=126 => emit(c as u32, 1),
        _ => (),
    }
}
pub fn control(op: u16, a: u16, b: u16) -> u16 {
    match op {
        1 => output(a),
        2 => mode_set(field(6)),
        3 => {
            if a < 80 && b < rows() {
                set(8, a);
                set(10, b);
                if field(6) == 7 {
                    video(0x0200, 0, 0, (b << 8) | a);
                }
            }
        }
        4 => {
            set(14, a & 15);
            set(16, b & 15)
        }
        5 => return rows(),
        6 => mode_set(a),
        7 => return field(6),
        8 => {
            set(16, b);
            rectangle(a << 8, 0x150)
        }
        9 => theme(),
        10 => rectangle(a, b),
        11 => set(24, if a == 2 && field(6) != 7 { 2 } else { 1 }),
        12 => {
            let mode = field(6);
            let x = field(8);
            let y = field(10) * 16 + 8;
            if a <= 80 - x && mode != 7 {
                let mut i = 0;
                while i < a {
                    solid(mode, addr(mode, y, x + i), b);
                    i += 1;
                }
                if mode != 6 {
                    planar(15)
                }
            }
        }
        13 => unsafe { serial_char(a) },
        15 => {
            let k = unsafe { bios_scan() };
            return if k & 255 == 0 || k & 255 == 224 {
                256 + (k >> 8)
            } else {
                k & 255
            };
        }
        _ => (),
    }
    0
}
