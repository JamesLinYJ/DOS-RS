//! Render 80×25 DOS text into the GOP linear framebuffer after firmware exit.
use crate::kernel_cell::KernelCell;
use font8x8::{
    BASIC_FONTS, BLOCK_FONTS, BOX_FONTS, GREEK_FONTS, LATIN_FONTS, MISC_FONTS, UnicodeFonts,
};
/// The guest selects glyphs by OEM byte. Map the upper half of IBM code page 437.
fn glyph(byte: u8) -> [u8; 8] {
    const EXTENDED: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ',
        'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú',
        'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡',
        '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
        '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘',
        '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ',
        '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²',
        '■', '\u{a0}',
    ];
    let ch = if byte < 128 {
        char::from(byte)
    } else {
        EXTENDED[usize::from(byte) - 128]
    };
    BASIC_FONTS
        .get(ch)
        .or_else(|| BOX_FONTS.get(ch))
        .or_else(|| BLOCK_FONTS.get(ch))
        .or_else(|| LATIN_FONTS.get(ch))
        .or_else(|| GREEK_FONTS.get(ch))
        .or_else(|| MISC_FONTS.get(ch))
        .unwrap_or([0; 8])
}
struct Frame {
    base: usize,
    stride: usize,
    rgb: bool,
    origin: usize,
}
static FRAME: KernelCell<Option<Frame>> = KernelCell::new(None);
pub(crate) fn initialize(
    base: usize,
    len: usize,
    width: usize,
    height: usize,
    stride: usize,
    rgb: bool,
) {
    if width >= 640
        && height >= 400
        && stride >= width
        && base != 0
        && base % 4 == 0
        && stride
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| n <= len && base.checked_add(n).is_some())
    {
        // The validated framebuffer includes the entire visible surface.
        for at in 0..stride * height {
            unsafe {
                (base as *mut u32).add(at).write_volatile(0);
            }
        }
        let origin = (height - 400) / 2 * stride + (width - 640) / 2;
        *FRAME.borrow() = Some(Frame {
            base,
            stride,
            rgb,
            origin,
        });
    }
}
pub(crate) fn vga_text() {
    *FRAME.borrow() = Some(Frame {
        base: 0xb8000,
        stride: 0,
        rgb: false,
        origin: 0,
    });
}
pub(super) fn cell(index: usize, word: u16, cursor: bool) {
    if index >= 2000 {
        return;
    }
    let fb = FRAME.borrow();
    let Some(f) = fb.as_ref() else { return };
    if f.stride == 0 {
        // BIOS mode 03h exposes 80 by 25 character/attribute words.
        unsafe {
            (f.base as *mut u16).add(index).write_volatile(if cursor {
                word ^ 0x7700
            } else {
                word
            });
        }
        return;
    }
    let palette = [
        0x000000u32,
        0x0000aa,
        0x00aa00,
        0x00aaaa,
        0xaa0000,
        0xaa00aa,
        0xaa5500,
        0xaaaaaa,
        0x555555,
        0x5555ff,
        0x55ff55,
        0x55ffff,
        0xff5555,
        0xff55ff,
        0xffff55,
        0xffffff,
    ];
    let pixel = |v: usize| {
        let color = palette[v];
        if f.rgb {
            ((color & 255) << 16) | (color & 0xff00) | (color >> 16)
        } else {
            color
        }
    };
    let fg = pixel(usize::from((word >> 8) & 15));
    let bg = pixel(usize::from((word >> 12) & 7));
    let rows = glyph(word as u8);
    let first = f.origin + (index / 80) * 16 * f.stride + (index % 80) * 8;
    for y in 0..16 {
        let bits = if cursor && y >= 14 { 0xff } else { rows[y / 2] };
        for x in 0..8 {
            let color = if bits & (1 << x) != 0 { fg } else { bg };
            // The GOP geometry was checked at handoff; index is an 80×25 cell.
            unsafe {
                ((f.base as *mut u32).add(first + y * f.stride + x)).write_volatile(color);
            }
        }
    }
}
