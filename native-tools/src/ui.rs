use crate::media_set::{self, CONFIG_BUFFER, Package};
use crate::{argument, capacity, install, partition, put, screen, text};
use dosrs_realmode::{
    self as pc, byte,
    system::{File, Media, dw, w},
};
const MSG: u16 = 0x3500;
const COUNT: u16 = 103;
const PAPER: u16 = 7;
const DARK: u16 = 1;
const INK: u16 = 0;
const TEAL: u16 = 3;
const WHITE: u16 = 15;
struct Dialog {
    media: Media,
    lang: File,
    locale: u16,
    rows: u16,
    setup: bool,
}
fn pos(x: u16, y: u16) {
    screen(3, x, y);
}
fn ink(fg: u16, bg: u16) {
    screen(4, fg, bg);
}
fn rect(x: u16, y: u16, width: u16, height: u16, bg: u16) {
    ink(INK, bg);
    screen(10, (y << 8) | x, (height << 8) | width);
}
fn hotkey() -> u16 {
    screen(15, 0, 0)
}
fn serial(s: &str) {
    let mut i = 0;
    while i < s.len() {
        screen(13, s.as_bytes()[i] as u16, 0);
        i += 1;
    }
}
impl Dialog {
    fn localized(&self, locale: u16, id: u16, log: bool) {
        let entry = (locale * COUNT + id) as u32 * 6;
        if !self.media.fetch(&self.lang, entry, 6, MSG, 0) {
            return;
        }
        let offset = dw(MSG, 0);
        let n = w(MSG, 4);
        if n > 256 || !self.media.fetch(&self.lang, offset, n, MSG, 0) {
            return;
        }
        let mut j = 0;
        while j < n {
            let c = byte(MSG, j);
            if log {
                screen(13, c, 0);
            } else {
                put(c)
            }
            j += 1;
        }
    }
    fn msg(&self, id: u16) {
        self.localized(self.locale, id, false)
    }
    fn marker(&self, id: u16) {
        self.localized(self.locale, id, true)
    }
    fn prepare_media(&self, package: &mut Package) -> bool {
        if self.media.unit >= 128 {
            return true;
        }
        if self.media.lookup(b"DISK1   ID ").is_none() {
            return true;
        }
        let Some(id) = media_set::identity(&self.media, 1) else {
            return false;
        };
        let unit = self.media.unit;
        if pc::floppies() > 1 {
            if let Some(disk) = media_set::matching(unit ^ 1, 2, &id) {
                return package.stage(&disk);
            }
        }
        // Draw instructions with disk 1 mounted, before its font can be removed.
        self.frame(3, 99);
        let y = self.body();
        self.note(y, 100);
        self.note(y + 2, 101);
        self.note(y + 4, 102);
        rect(2, y, 1, 1, TEAL);
        serial("Insert disk 2 > ");
        let mut ok = false;
        loop {
            let k = hotkey();
            if k == 27 {
                break;
            }
            if k != 13 {
                continue;
            }
            if let Some(disk) = media_set::matching(unit, 2, &id) {
                ok = package.stage(&disk);
                if ok {
                    break;
                }
            }
            rect(2, y, 1, 1, 4);
            serial("Wrong disk 2 > ");
        }
        rect(2, y, 1, 1, PAPER);
        rect(2, y + 2, 1, 1, TEAL);
        serial("Restore disk 1 > ");
        loop {
            if media_set::matching(unit, 1, &id).is_some() {
                return ok;
            }
            let k = hotkey();
            if k == 27 {
                ok = false;
            } else if k == 13 && media_set::matching(unit, 1, &id).is_none() {
                rect(2, y + 2, 1, 1, 4);
                serial("Wrong disk 1 > ");
            }
        }
    }
    fn roomy(&self) -> bool {
        self.rows >= 20
    }
    fn body(&self) -> u16 {
        if self.rows >= 27 {
            9
        } else if self.roomy() {
            6
        } else {
            3
        }
    }
    fn frame(&self, step: u16, title: u16) {
        rect(0, 0, 80, self.rows, PAPER);
        let tall = self.roomy();
        let top = if tall { 3 } else { 1 };
        rect(0, 0, 80, top, DARK);
        pos(4, 0);
        ink(WHITE, DARK);
        if tall {
            screen(11, 2, 0);
        }
        text("DOS");
        screen(11, 1, 0);
        pos(if tall { 13 } else { 9 }, if tall { 1 } else { 0 });
        ink(11, DARK);
        text("/ ");
        self.msg(if self.setup { 54 } else { 55 });
        pos(68, if tall { 1 } else { 0 });
        ink(WHITE, DARK);
        put(48 + step);
        text(" / 4");
        if tall {
            let mut i = 0;
            while i < 4 {
                let bg = if i + 1 == step {
                    TEAL
                } else if i + 1 < step {
                    10
                } else {
                    8
                };
                rect(i * 20, top, 20, 1, bg);
                pos(i * 20 + 2, top);
                ink(if i + 1 < step { INK } else { WHITE }, bg);
                self.msg(56 + i);
                i += 1;
            }
        }
        pos(
            4,
            if self.rows >= 27 {
                6
            } else if tall {
                4
            } else {
                1
            },
        );
        ink(INK, PAPER);
        if self.rows >= 27 {
            screen(11, 2, 0);
        }
        self.msg(title);
        screen(11, 1, 0);
    }
    fn footer(&self, back: u16, accept: u16) {
        let height = if self.rows >= 27 { 3 } else { 1 };
        let start = self.rows - height;
        let y = start + (height >> 1);
        rect(0, start, 80, height, DARK);
        if back != 0 {
            pos(4, y);
            ink(WHITE, DARK);
            text("Esc  ");
            self.msg(back);
        }
        if accept != 0 {
            rect(52, start, 28, height, TEAL);
            pos(54, y);
            ink(WHITE, TEAL);
            text("Enter  ");
            self.msg(accept)
        }
    }
    fn note(&self, y: u16, id: u16) {
        pos(4, y);
        ink(INK, PAPER);
        self.msg(id)
    }
    fn item(&self, x: u16, y: u16, width: u16, num: u16, selected: bool) {
        let bg = if selected { TEAL } else { WHITE };
        let h = if self.rows >= 27 { 3 } else { 1 };
        rect(x, y, width, h, bg);
        let line = y + (h >> 1);
        pos(x + 1, line);
        ink(if selected { WHITE } else { TEAL }, bg);
        put(if selected { 62 } else { 32 });
        put(49 + num);
        pos(x + 5, line);
        ink(if selected { WHITE } else { INK }, bg);
    }
    fn line(&self, y: u16, label: u16) {
        pos(6, y);
        ink(8, WHITE);
        self.msg(label);
        pos(31, y);
        ink(INK, WHITE);
    }
    fn license(&self) {
        let Some(f) = self.media.lookup(b"LICENSE TXT") else {
            return;
        };
        if f.size > 2048 {
            return;
        }
        if !self.media.fetch(&f, 0, f.size as u16, CONFIG_BUFFER, 0) {
            return;
        }
        let mut page = 0;
        loop {
            self.frame(2, 89);
            let top = self.body();
            let lines = self.rows - top - if self.rows >= 27 { 5 } else { 2 };
            let mut posn = 0;
            let mut line = 0;
            let mut col = 0;
            while posn < f.size as u16 && line < (page + 1) * lines {
                let c = byte(CONFIG_BUFFER, posn);
                posn += 1;
                if c == 13 {
                    continue;
                }
                if c == 10 {
                    line += 1;
                    col = 0;
                    continue;
                }
                if line >= page * lines {
                    if col == 0 {
                        pos(4, top + line - page * lines);
                        ink(INK, PAPER);
                    }
                    put(c)
                }
                col += 1;
                if col >= 72 {
                    line += 1;
                    col = 0;
                }
            }
            self.footer(60, 62);
            serial("License > ");
            let k = hotkey();
            if k == 27 {
                break;
            }
            if matches!(k, 0x149 | 0x148 | 0x14b) {
                if page > 0 {
                    page -= 1
                }
            } else if matches!(k, 13 | 32 | 0x151 | 0x150 | 0x14d) {
                if posn >= f.size as u16 {
                    break;
                }
                page += 1;
            }
        }
    }
    fn finish(&self, id: u16, ok: bool) {
        self.frame(4, if !ok { 72 } else { 67 });
        let y = self.body() + 1;
        rect(3, y, 74, if self.roomy() { 6 } else { 3 }, WHITE);
        rect(
            3,
            y,
            1,
            if self.roomy() { 6 } else { 3 },
            if ok { TEAL } else { 4 },
        );
        pos(6, y + 1);
        ink(INK, WHITE);
        self.msg(id);
        self.footer(0, 68);
        serial("DOS > ");
        let _ = hotkey();
        screen(2, 0, 0);
    }
    fn flow<A: Operations>(&mut self) -> u16 {
        let mut step = 1;
        let mut disk = 128;
        let mut action = 1;
        let mut target = partition::inspect();
        let mut bytes = 0;
        let mut focus = self.locale;
        let mut package = Package::new(self.media);
        let mut settings = crate::startup::Settings::defaults();
        loop {
            let body = self.body();
            let tall = self.roomy();
            if step == 1 {
                self.frame(step, 12);
                let mut i = 0;
                while i < 8 {
                    let x = 4 + (i & 1) * 37;
                    let y = body + (i >> 1) * if self.rows >= 27 { 4 } else { 2 };
                    self.item(x, y, 35, i, focus == i);
                    if screen(7, 0, 0) == 7 {
                        text(match i {
                            0 => "Deutsch",
                            1 => "English",
                            2 => "Espanol",
                            3 => "Francais",
                            4 => "Japanese",
                            5 => "Korean",
                            6 => "Chinese (Simplified)",
                            _ => "Chinese (Traditional)",
                        });
                    } else {
                        self.localized(i, 0, false)
                    }
                    i += 1;
                }
                if tall {
                    self.note(self.rows - 5, 69);
                }
                self.footer(61, 62);
                serial("1-8 > ");
                let k = hotkey();
                if k == 27 {
                    screen(2, 0, 0);
                    self.msg(6);
                    text("\r\n");
                    return 0;
                }
                if let Some(i) = choose(k, 8, &mut focus, 2) {
                    self.locale = if screen(7, 0, 0) == 7 { 1 } else { i };
                    settings.language = self.locale;
                    step = 2;
                    focus = 0
                }
            } else if step == 2 {
                self.frame(step, 13);
                let count = byte(0x40, 0x75);
                let count = if count > 8 { 8 } else { count };
                let mut i = 0;
                while i < count {
                    pc::store(pc::COMMAND, 260, 128 + i);
                    let y = body + (i >> 1) * if self.rows >= 27 { 4 } else { 2 };
                    self.item(4 + (i & 1) * 37, y, 35, i, i == focus);
                    text("BIOS ");
                    install::number((128 + i) as u32);
                    text("   ");
                    install::number(capacity() >> 11);
                    text(" MiB");
                    i += 1;
                }
                self.footer(60, 62);
                self.note(self.rows - if self.rows >= 27 { 5 } else { 2 }, 98);
                self.marker(44);
                if count == 0 {
                    self.note(body, 47);
                    let _ = hotkey();
                    step = 1;
                    focus = self.locale;
                    continue;
                }
                let k = hotkey();
                if matches!(k, 76 | 108 | 0x13b) {
                    self.license();
                    continue;
                }
                if k == 27 {
                    step = 1;
                    focus = self.locale;
                    continue;
                }
                if let Some(index) = choose(k, count, &mut focus, 2) {
                    disk = 128 + index;
                    pc::store(pc::COMMAND, 260, disk);
                    if self.media.unit == disk {
                        self.note(self.rows - 3, 30);
                        let _ = hotkey();
                        continue;
                    }
                    target = partition::inspect();
                    step = 3;
                    focus = 0;
                }
            } else if step == 3 {
                self.frame(step, 14);
                let state = target.state;
                if !matches!(
                    state,
                    partition::NEW | partition::SPACE | partition::PREPARED | partition::INSTALLED
                ) {
                    self.note(
                        body,
                        match state {
                            partition::GPT => 35,
                            partition::INVALID => 36,
                            partition::SMALL => 37,
                            partition::EXTENDED => 38,
                            partition::FULL => 39,
                            _ => 7,
                        },
                    );
                    self.footer(60, 60);
                    let _ = hotkey();
                    step = 2;
                    focus = disk - 128;
                    continue;
                }
                rect(4, body, 72, if tall { 4 } else { 2 }, WHITE);
                self.line(body, 40);
                text("#");
                install::number(target.slot as u32 + 1);
                text("   ");
                install::number(target.size >> 11);
                text(" MiB");
                self.line(body + if tall { 2 } else { 1 }, 75);
                text("LBA ");
                install::number(target.first);
                text(" - ");
                install::number(target.first + target.size - 1);
                let options = if self.setup && state == partition::INSTALLED {
                    2
                } else {
                    1
                };
                if !self.setup && matches!(state, partition::PREPARED | partition::INSTALLED) {
                    self.note(body + 4, if state == partition::INSTALLED { 21 } else { 8 });
                    self.footer(60, 60);
                    let _ = hotkey();
                    step = 2;
                    focus = disk - 128;
                    continue;
                }
                let mut i = 0;
                while i < options {
                    self.item(
                        4,
                        body + if tall { 6 } else { 3 } + i * if self.rows >= 27 { 4 } else { 2 },
                        72,
                        i,
                        i == focus,
                    );
                    self.msg(if options == 2 {
                        if i == 0 { 17 } else { 18 }
                    } else if self.setup {
                        16
                    } else if target.shared {
                        66
                    } else {
                        3
                    });
                    i += 1;
                }
                if target.shared {
                    self.note(self.rows - if self.rows >= 27 { 5 } else { 2 }, 31)
                }
                self.footer(60, 62);
                self.marker(45);
                let k = hotkey();
                if k == 27 {
                    step = 2;
                    focus = disk - 128;
                    continue;
                }
                if let Some(index) = choose(k, options, &mut focus, 1) {
                    action = if state == partition::INSTALLED {
                        2 + index
                    } else {
                        1
                    };
                    self.note(self.rows - if self.rows >= 27 { 5 } else { 2 }, 48);
                    if self.setup && !self.prepare_media(&mut package) {
                        self.finish(6, false);
                        return 1;
                    }
                    bytes = A::preflight(&package);
                    if bytes == 0 {
                        self.finish(20, false);
                        return 1;
                    }
                    step = if self.setup && action == 1 { 5 } else { 4 };
                }
            } else if step == 5 {
                self.frame(3, 80);
                let mut i = 0;
                let gap = if self.rows >= 27 { 3 } else { 1 };
                rect(4, body, 72, gap * 4 + 1, WHITE);
                while i < 4 {
                    let y = body + 1 + i * gap;
                    pos(6, y);
                    ink(TEAL, WHITE);
                    put(49 + i);
                    pos(10, y);
                    ink(INK, WHITE);
                    self.msg(match i {
                        0 => 81,
                        1 => 82,
                        2 => 83,
                        _ => 96,
                    });
                    pos(35, y);
                    match i {
                        0 => text(settings.mode()),
                        1 => self.msg(if settings.cache { 87 } else { 88 }),
                        2 => self.msg(if settings.cd { 87 } else { 88 }),
                        _ => self.msg(84 + settings.batch),
                    }
                    i += 1;
                }
                if tall {
                    pos(4, self.rows - 5);
                    ink(INK, PAPER);
                    text("CONFIG.SYS    AUTOEXEC.BAT");
                }
                self.footer(60, 62);
                serial("Config > ");
                let k = hotkey();
                match k {
                    27 => step = 3,
                    13 => step = 4,
                    49 => settings.display = (settings.display + 1) % 5,
                    50 => settings.cache = !settings.cache,
                    51 => settings.cd = !settings.cd,
                    52 => {
                        settings.batch = (settings.batch + 1) % 3;
                        if settings.batch == 2 {
                            if let Some(f) = self.media.lookup(b"AUTOEXECBAT") {
                                if f.size == 0 || f.size > 2048 {
                                    settings.batch = 0
                                }
                            } else {
                                settings.batch = 0;
                                self.note(self.rows - 5, 93);
                                let _ = hotkey();
                            }
                        }
                    }
                    _ => (),
                }
            } else {
                let gap = if self.rows >= 27 { 2 } else { 1 };
                self.frame(step, 19);
                rect(4, body, 72, 4 * gap + 1, WHITE);
                rect(4, body, 1, 4 * gap + 1, TEAL);
                self.line(body + 1, 25);
                self.msg(57);
                text(" ");
                install::number((disk - 127) as u32);
                text("   BIOS ");
                install::number(disk as u32);
                self.line(body + 1 + gap, 28);
                text("#");
                install::number(target.slot as u32 + 1);
                text(if target.size >= 70000 {
                    "   MBR / FAT32   "
                } else {
                    "   MBR / FAT16   "
                });
                install::number(target.size >> 11);
                text(" MiB");
                self.line(body + 1 + 2 * gap, 75);
                install::number(target.first);
                text(" - ");
                install::number(target.first + target.size - 1);
                self.line(body + 1 + 3 * gap, 27);
                install::number((bytes + 1023) >> 10);
                text(" KiB");
                let accept = if action == 2 {
                    64
                } else if action == 3 {
                    65
                } else if self.setup {
                    63
                } else {
                    66
                };
                self.note(
                    body + 4 * gap + 2,
                    if target.shared {
                        31
                    } else if action == 2 {
                        17
                    } else if action == 3 {
                        18
                    } else if self.setup {
                        4
                    } else {
                        3
                    },
                );
                if tall {
                    self.note(self.rows - 5, 74)
                }
                self.footer(60, accept);
                self.marker(46);
                let k = hotkey();
                if k == 27 {
                    step = if self.setup && action == 1 { 5 } else { 3 };
                    focus = 0;
                    continue;
                }
                if k != 13 {
                    continue;
                }
                if !target.same(&partition::inspect()) {
                    self.finish(7, false);
                    return 1;
                }
                self.frame(
                    4,
                    if action == 2 {
                        64
                    } else if action == 3 {
                        65
                    } else if self.setup {
                        70
                    } else {
                        66
                    },
                );
                if action == 1 && matches!(target.state, partition::NEW | partition::SPACE) {
                    self.note(body, 33);
                    if !install::partition(&package, &target) {
                        self.finish(34, false);
                        return 1;
                    }
                    target = partition::inspect();
                }
                if action == 1 && self.setup {
                    rect(4, body, 72, 3, PAPER);
                    self.note(body + 2, 50);
                    set_total(
                        bytes
                            + settings.render(&self.media, 0, CONFIG_BUFFER) as u32
                            + settings.render(&self.media, 1, CONFIG_BUFFER) as u32,
                    );
                    if !A::write(&package, &target, &settings) {
                        self.finish(9, false);
                        return 1;
                    }
                }
                if self.setup {
                    rect(4, body, 72, 3, PAPER);
                    self.note(body + 2, 22);
                    set_total(bytes);
                    pc::cache::reset();
                    if !A::verify(&package, &target, action == 3) {
                        self.finish(if action == 2 { 49 } else { 9 }, false);
                        return 1;
                    }
                }
                self.finish(
                    if !self.setup {
                        8
                    } else if action == 3 {
                        43
                    } else if action == 2 {
                        23
                    } else if target.shared {
                        32
                    } else {
                        11
                    },
                    true,
                );
                return 0;
            }
        }
    }
}
fn choose(k: u16, count: u16, focus: &mut u16, cols: u16) -> Option<u16> {
    if k == 13 {
        return Some(*focus);
    }
    if k >= 49 && k < 49 + count {
        return Some(k - 49);
    }
    let delta = match k {
        0x148 => {
            if count > cols {
                count - cols
            } else {
                0
            }
        }
        0x150 => cols,
        0x14b => count - 1,
        0x14d | 9 => 1,
        _ => 0,
    };
    *focus = (*focus + delta) % count;
    None
}
const PROGRESS: u16 = 0x37d0;
pub fn set_total(n: u32) {
    pc::system::sd(PROGRESS, 0, n);
    pc::system::sd(PROGRESS, 4, 0);
}
fn percent(mut n: u32, mut total: u32) -> u16 {
    if total == 0 {
        return 0;
    }
    if n >= total {
        return 100;
    }
    while total > 600 {
        total = (total + 1) >> 1;
        n >>= 1;
    }
    (n as u16) * 100 / (total as u16)
}
fn bar(y: u16, p: u16) {
    let width = p * 55 / 100;
    if width != 0 {
        rect(5, y, width, 1, TEAL);
    }
    pos(62, y);
    ink(INK, WHITE);
    install::number(p as u32);
    text("%  ");
}
pub fn progress_begin(index: u16, total: u16, name: &[u8], size: u32) {
    let rows = screen(5, 0, 0);
    let y = if rows >= 27 {
        17
    } else if rows >= 20 {
        12
    } else {
        6
    };
    pc::system::sw(PROGRESS, 8, y);
    pc::system::sd(PROGRESS, 10, size);
    pc::system::sw(PROGRESS, 14, 65535);
    rect(4, y, 72, 4, WHITE);
    pos(5, y);
    ink(INK, WHITE);
    let mut i = 0;
    while i < name.len() {
        if i == 8 {
            put(46)
        }
        if name[i] != 32 {
            put(name[i] as u16)
        }
        i += 1;
    }
    pos(45, y);
    install::number((size + 1023) >> 10);
    text(" KiB   ");
    install::number(index as u32 + 1);
    text("/");
    install::number(total as u32);
    pos(5, y + 2);
    ink(INK, WHITE);
    text("TOTAL");
    progress_bytes(0);
}
pub fn progress_bytes(done: u32) {
    let pct = percent(done, dw(PROGRESS, 10));
    if pct == w(PROGRESS, 14) {
        return;
    }
    pc::system::sw(PROGRESS, 14, pct);
    let y = w(PROGRESS, 8);
    bar(y + 1, pct);
    let all = dw(PROGRESS, 4) + done;
    bar(y + 3, percent(all, dw(PROGRESS, 0)));
}
pub fn progress(_index: u16, _total: u16, _name: &[u8]) {
    let size = dw(PROGRESS, 10);
    progress_bytes(size);
    pc::system::sd(PROGRESS, 4, dw(PROGRESS, 4) + size);
}
pub trait Operations {
    const SYSTEM: bool;
    fn preflight(media: &Package) -> u32;
    fn write(
        media: &Package,
        target: &partition::Target,
        settings: &crate::startup::Settings,
    ) -> bool;
    fn verify(media: &Package, target: &partition::Target, repair: bool) -> bool;
}
pub struct Setup;
impl Operations for Setup {
    const SYSTEM: bool = true;
    fn preflight(m: &Package) -> u32 {
        install::preflight(m)
    }
    fn write(m: &Package, t: &partition::Target, s: &crate::startup::Settings) -> bool {
        install::install(m, t, s)
    }
    fn verify(m: &Package, t: &partition::Target, r: bool) -> bool {
        install::verify(m, t, r)
    }
}
pub struct Fdisk;
impl Operations for Fdisk {
    const SYSTEM: bool = false;
    fn preflight(m: &Package) -> u32 {
        if let Some(f) = m.boot.lookup(b"BOOT    BIN") {
            if f.size == 1024 {
                return 1024;
            }
        }
        0
    }
    fn write(_: &Package, _: &partition::Target, _: &crate::startup::Settings) -> bool {
        false
    }
    fn verify(_: &Package, _: &partition::Target, _: bool) -> bool {
        false
    }
}
pub fn run<A: Operations>() -> u16 {
    let setup = A::SYSTEM;
    let source = pc::boot_unit();
    let saved = pc::hard_unit();
    let Some(media) = Media::device(source) else {
        text("Boot volume unavailable\r\n");
        return 1;
    };
    if argument(1, "/STATUS") {
        install::report();
        return 0;
    }
    let Some(mut lang) = media.lookup(b"LANG    DAT") else {
        text("Missing LANG.DAT\r\n");
        return 1;
    };
    let original = screen(7, 0, 0);
    if argument(1, "/CGA") {
        screen(6, 6, 0);
    } else if argument(1, "/EGA") {
        screen(6, 16, 0);
    } else if argument(1, "/MONO") {
        screen(6, 7, 0);
    }
    media.check_extent(&mut lang);
    screen(9, 0, 0);
    let locale = if screen(7, 0, 0) == 7 { 1 } else { 6 };
    let mut d = Dialog {
        media,
        lang,
        locale,
        rows: screen(5, 0, 0),
        setup,
    };
    let result = d.flow::<A>();
    if original != screen(7, 0, 0) {
        screen(6, original, 0);
    }
    pc::store(pc::COMMAND, 260, saved);
    result
}
