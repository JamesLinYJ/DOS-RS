//! Polled PC-compatible devices used by DOS names and the guest BIOS.
use crate::arch::{inb, outb};
use crate::kernel_cell::KernelCell;

pub(crate) const UARTS: [u16; 2] = [0x3f8, 0x2f8];

pub(crate) fn serial_status(base: u16) -> u16 {
    u16::from_be_bytes([inb(base + 5), inb(base + 6)])
}

pub(crate) fn configure_serial(base: u16, config: u8) {
    let divisor = [1047u16, 768, 384, 192, 96, 48, 24, 12][usize::from(config >> 5)];
    let [low, high] = divisor.to_le_bytes();
    outb(base + 1, 0);
    outb(base + 3, 0x80);
    outb(base, low);
    outb(base + 1, high);
    outb(base + 3, config & 0x1f);
    outb(base + 4, 3);
}

pub(crate) fn serial_write(base: u16, byte: u8) -> bool {
    for _ in 0..4096 {
        let status = inb(base + 5);
        if status == 0xff {
            break;
        }
        if status & 0x20 != 0 {
            outb(base, byte);
            return true;
        }
    }
    false
}

pub(crate) fn serial_read(base: u16) -> Option<u8> {
    let status = inb(base + 5);
    (status != 0xff && status & 1 != 0).then(|| inb(base))
}

pub(crate) fn printer_status() -> u8 {
    // The PC status register and BIOS use opposite polarity for ACK and ERROR.
    (inb(0x379) ^ 0x48) & 0xf8
}

pub(crate) fn print_byte(byte: u8) -> bool {
    for _ in 0..4096 {
        let status = inb(0x379);
        if status == 0xff || status & 0x20 != 0 {
            break;
        }
        if status & 0x80 != 0 {
            let control = (inb(0x37a) | 0x0c) & !0x21;
            outb(0x378, byte);
            outb(0x37a, control);
            outb(0x37a, control | 1);
            outb(0x37a, control);
            return true;
        }
    }
    false
}

#[derive(Default)]
struct Keyboard {
    pending: Option<u16>,
    shift: u8,
    caps: bool,
    alt: bool,
    extended: bool,
}
static KEYBOARD: KernelCell<Keyboard> = KernelCell::new(Keyboard {
    pending: None,
    shift: 0,
    caps: false,
    alt: false,
    extended: false,
});

impl Keyboard {
    fn decode(&mut self, code: u8) -> Option<u16> {
        if code == 0xe0 {
            self.extended = true;
            return None;
        }
        let extended = core::mem::take(&mut self.extended);
        let scan = code & 0x7f;
        let down = code == scan;
        if !extended && matches!(scan, 0x2a | 0x36) {
            let bit = if scan == 0x2a { 2 } else { 1 };
            if down {
                self.shift |= bit;
            } else {
                self.shift &= !bit;
            }
            return None;
        }
        if scan == 0x38 {
            self.alt = down;
            return None;
        }
        if self.alt && down && (0x3b..=0x3e).contains(&scan) {
            return Some(u16::from(scan + 0x2d) << 8);
        }
        if !down {
            return None;
        }
        if scan == 0x3a {
            self.caps = !self.caps;
            return None;
        }
        let plain = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\r\0asdfghjkl;'\x60\0\\zxcvbnm,./\0*\0 ";
        let upper = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";
        let ascii = if extended || scan >= 0x3b {
            0
        } else {
            let ch = *plain.get(usize::from(scan))?;
            if ch == 0 {
                return None;
            }
            if (self.shift != 0) ^ (ch.is_ascii_alphabetic() && self.caps) {
                upper[usize::from(scan)]
            } else {
                ch
            }
        };
        Some(u16::from_be_bytes([scan, ascii]))
    }
}

fn physical_key(consume: bool) -> Option<u16> {
    let mut keyboard = KEYBOARD.borrow();
    while keyboard.pending.is_none() {
        let status = inb(0x64);
        if status & 0x21 != 1 {
            break;
        }
        let code = inb(0x60);
        keyboard.pending = keyboard.decode(code);
    }
    if consume {
        keyboard.pending.take()
    } else {
        keyboard.pending
    }
}

pub(crate) const TERMINALS: usize = 4;
const COLS: usize = 80;
const ROWS: usize = 25;
const SPACE: u16 = 0x0720;

struct TextScreen {
    cells: [u16; COLS * ROWS],
    cursor: usize,
    attribute: u8,
    keys: [u16; 32],
    head: usize,
    count: usize,
}
impl TextScreen {
    const fn new() -> Self {
        Self {
            cells: [SPACE; COLS * ROWS],
            cursor: 0,
            attribute: 7,
            keys: [0; 32],
            head: 0,
            count: 0,
        }
    }
    fn key(&mut self, consume: bool) -> Option<u16> {
        if self.count == 0 {
            return None;
        }
        let key = self.keys[self.head];
        if consume {
            self.head = (self.head + 1) % self.keys.len();
            self.count -= 1;
        }
        Some(key)
    }
    fn enqueue(&mut self, key: u16) {
        if self.count < self.keys.len() {
            self.keys[(self.head + self.count) % self.keys.len()] = key;
            self.count += 1;
        }
    }
    fn paint(&self, index: usize) {
        super::screen::cell(index, self.cells[index], false);
    }
    fn cursor(&self) {
        super::screen::cell(self.cursor, self.cells[self.cursor], true);
    }
    fn redraw(&self) {
        for i in 0..self.cells.len() {
            self.paint(i);
        }
        self.cursor();
    }
    fn write(&mut self, byte: u8, visible: bool) {
        if visible {
            self.paint(self.cursor);
        }
        match byte {
            b'\r' => self.cursor -= self.cursor % COLS,
            b'\n' => self.cursor += COLS,
            8 => self.cursor = self.cursor.saturating_sub(1),
            b'\t' => self.cursor += 8 - self.cursor % 8,
            0..=31 => return,
            _ => {
                self.cells[self.cursor] = u16::from_be_bytes([self.attribute, byte]);
                if visible {
                    self.paint(self.cursor);
                }
                self.cursor += 1;
            }
        }
        if self.cursor >= self.cells.len() {
            self.cells.copy_within(COLS.., 0);
            self.cells[(ROWS - 1) * COLS..].fill(u16::from_be_bytes([self.attribute, b' ']));
            self.cursor -= COLS;
            if visible {
                self.redraw();
            }
        } else if visible {
            self.cursor();
        }
    }
}

struct Consoles {
    screens: [TextScreen; TERMINALS],
    foreground: usize,
    running: usize,
    multiplexed: bool,
    initialized: bool,
}
static CONSOLES: KernelCell<Consoles> = KernelCell::new(Consoles {
    screens: [const { TextScreen::new() }; TERMINALS],
    foreground: 0,
    running: 0,
    multiplexed: false,
    initialized: false,
});

pub(crate) fn color(attribute: u8) -> u8 {
    let mut c = CONSOLES.borrow();
    let id = c.running;
    core::mem::replace(&mut c.screens[id].attribute, attribute)
}
/// Use the shell accent only while the terminal still uses the default attribute.
pub(crate) fn prompt_color() -> u8 {
    let mut c = CONSOLES.borrow();
    let id = c.running;
    let s = &mut c.screens[id];
    let original = s.attribute;
    if original == 7 {
        s.attribute = 0x0a;
    }
    original
}
pub(crate) fn recolor(attribute: u8) {
    let mut c = CONSOLES.borrow();
    let id = c.running;
    let visible = id == c.foreground;
    let screen = &mut c.screens[id];
    screen.attribute = attribute;
    for cell in &mut screen.cells {
        *cell = (*cell & 255) | (u16::from(attribute) << 8);
    }
    if visible {
        screen.redraw();
    }
}
pub(crate) fn position(row: u8, col: u8) {
    let mut c = CONSOLES.borrow();
    let id = c.running;
    let visible = id == c.foreground;
    let s = &mut c.screens[id];
    if visible {
        s.paint(s.cursor);
    }
    s.cursor = usize::from(row.min(24)) * 80 + usize::from(col.min(79));
    if visible {
        s.cursor();
    }
}
pub(crate) fn enable_terminals() {
    CONSOLES.borrow().multiplexed = true;
}
pub(crate) fn route_output(id: usize) {
    CONSOLES.borrow().running = id;
}
pub(crate) fn poll_keyboard(mut raw_input: impl FnMut(usize, u8) -> bool) -> usize {
    let mut consoles = CONSOLES.borrow();
    let mut keyboard = KEYBOARD.borrow();
    for _ in 0..32 {
        if inb(0x64) & 0x21 != 1 {
            break;
        }
        let code = inb(0x60);
        let key = keyboard.decode(code);
        let id = consoles.foreground;
        if let Some(0x6800 | 0x6900 | 0x6a00 | 0x6b00) = key {
            // Release Alt in the outgoing guest before consuming its switch chord.
            raw_input(id, 0xb8);
            consoles.foreground = usize::from(code - 0x3b);
            consoles.screens[consoles.foreground].redraw();
        } else if !raw_input(id, code)
            && let Some(key) = key
        {
            consoles.screens[id].enqueue(key);
        }
    }
    consoles.foreground
}
pub(crate) fn key(consume: bool) -> Option<u16> {
    let mut consoles = CONSOLES.borrow();
    if consoles.multiplexed {
        let id = consoles.running;
        consoles.screens[id].key(consume)
    } else {
        physical_key(consume)
    }
}
pub(crate) fn guest_text_row(row: usize, bytes: &[u8]) {
    let mut consoles = CONSOLES.borrow();
    let id = consoles.running;
    let visible = id == consoles.foreground;
    let screen = &mut consoles.screens[id];
    for (col, pair) in bytes.chunks_exact(2).take(COLS).enumerate() {
        let i = row * COLS + col;
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        if screen.cells[i] != value {
            screen.cells[i] = value;
            if visible {
                screen.paint(i);
            }
        }
    }
}
pub(crate) fn display(byte: u8) {
    let mut consoles = CONSOLES.borrow();
    let id = consoles.running;
    let visible = id == consoles.foreground;
    if !consoles.initialized {
        consoles.screens[id].redraw();
        consoles.initialized = true;
    }
    consoles.screens[id].write(byte, visible);
}

// Guest IN/OUT is routed only to peripherals implemented by the PC backend.
pub(crate) fn peripheral_port(port: u16) -> bool {
    matches!(port, 0x2f8..=0x2ff | 0x3f8..=0x3ff | 0x378..=0x37a)
}

pub(crate) fn port_read(port: u16) -> u8 {
    if peripheral_port(port) {
        inb(port)
    } else {
        0xff
    }
}

pub(crate) fn port_write(port: u16, value: u8) {
    if peripheral_port(port) {
        outb(port, value);
    }
}

#[cfg(feature = "pc386")]
pub(crate) fn restore_text() {
    let consoles = CONSOLES.borrow();
    super::screen::clear_graphics();
    consoles.screens[consoles.foreground].redraw();
}
