mod dos;
mod gap_buffer;
mod history;
mod language;
use language::{Language, Message};
mod navigation;
type Document = gap_buffer::GapBuffer<dos::Segment>;
use crate::{input, put, screen, text};

unsafe extern "Rust" {
    fn edit_heap() -> u16;
    fn edit_far_io(fd: u16, seg: u16, offset: u16, n: u16, ax: u16) -> u16;
    fn arg(i: u16, j: u16) -> u16;
    fn edit_open(name: *const u8, ax: u16) -> u16;
    fn edit_io(fd: u16, p: *const u8, n: u16, ax: u16) -> u16;
}
fn close(fd: u16) -> bool {
    unsafe { edit_io(fd, core::ptr::null(), 0, 0x3e00) < 0x8000 }
}
fn load(path: &[u8], segment: u16, capacity: u16) -> Result<usize, u16> {
    let fd = unsafe { edit_open(path.as_ptr(), 0x3d00) };
    if fd == 0x8002 {
        return Ok(0);
    }
    if fd >= 0x8000 {
        return Err(fd);
    }
    let mut n = 0;
    loop {
        let room = capacity - n;
        let want = if room > 4096 { 4096 } else { room };
        if want == 0 {
            let mut extra = 0u8;
            let read = unsafe { edit_io(fd, &mut extra, 1, 0x3f00) };
            if !close(fd) || read != 0 {
                return Err(8);
            }
            return Ok(n as usize);
        }
        let got = unsafe { edit_far_io(fd, segment, n, want, 0x3f00) };
        if got > want {
            close(fd);
            return Err(got);
        }
        n += got;
        if got < want {
            if !close(fd) {
                return Err(5);
            }
            return Ok(n as usize);
        }
    }
}
fn save(path: &[u8], doc: &mut Document) -> bool {
    let mut fd = unsafe { edit_open(path.as_ptr(), 0x3d02) };
    if fd == 0x8002 {
        fd = unsafe { edit_open(path.as_ptr(), 0x3c00) };
    }
    if fd >= 0x8000 {
        return false;
    }
    let len = doc.text_length as u16;
    let seg = doc.flatten().base;
    let mut done = 0;
    while done < len {
        let n = if len - done > 4096 { 4096 } else { len - done };
        if unsafe { edit_far_io(fd, seg, done, n, 0x4000) } != n {
            close(fd);
            return false;
        }
        done += n;
    }
    let truncated = unsafe { edit_far_io(fd, seg, done, 0, 0x4000) } == 0;
    close(fd) && truncated
}
fn unit(d: &Document, at: usize) -> usize {
    let c = d.byte(at);
    if c == 13 && at + 1 < d.text_length && d.byte(at + 1) == 10 {
        return 2;
    }
    if c >= 129 && c <= 254 && at + 1 < d.text_length {
        let b = d.byte(at + 1);
        if b >= 48
            && b <= 57
            && at + 3 < d.text_length
            && d.byte(at + 2) >= 129
            && d.byte(at + 2) <= 254
            && d.byte(at + 3) >= 48
            && d.byte(at + 3) <= 57
        {
            return 4;
        }
        if b >= 64 && b != 127 && b <= 254 {
            return 2;
        }
    }
    1
}
fn previous(d: &Document, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    if d.byte(pos - 1) == 10 {
        return if pos >= 2 && d.byte(pos - 2) == 13 {
            pos - 2
        } else {
            pos - 1
        };
    }
    let mut p = start(d, pos);
    while p < pos {
        let next = p + unit(d, p);
        if next >= pos {
            return p;
        }
        p = next;
    }
    p
}
fn start(d: &Document, pos: usize) -> usize {
    let mut p = pos;
    while p > 0 && d.byte(p - 1) != 10 {
        p -= 1;
    }
    p
}
fn end(d: &Document, pos: usize) -> usize {
    let mut p = pos;
    while p < d.text_length && d.byte(p) != 13 && d.byte(p) != 10 {
        p += unit(d, p);
    }
    p
}
fn vertical(d: &Document, pos: usize, down: bool) -> usize {
    let begin = start(d, pos);
    let mut target = if down {
        let e = end(d, pos);
        if e == d.text_length {
            return pos;
        }
        e + unit(d, e)
    } else {
        if begin == 0 {
            return pos;
        }
        start(d, previous(d, begin))
    };
    let mut old = begin;
    let stop = end(d, target);
    while old < pos && target < stop {
        old += unit(d, old);
        target += unit(d, target)
    }
    target
}
fn draw(
    d: &Document,
    path: &[u8],
    posn: usize,
    status: Option<Message>,
    dirty: bool,
    lang: &Language,
) {
    let rows = screen(5, 0, 0);
    screen(4, 0, 7);
    screen(10, 0, (rows << 8) | 80);
    screen(4, 15, 1);
    screen(10, 0, 0x150);
    screen(3, 2, 0);
    text("Edit   ");
    if path[0] == 0 {
        lang.message(Message::Untitled)
    }
    let mut i = 0;
    while i < path.len() && path[i] != 0 {
        put(path[i] as u16);
        i += 1;
    }
    if dirty {
        text(" *")
    }
    let mut p = 0;
    let mut lineno = 1;
    let mut y = 0u16;
    let mut top = 0;
    while p < posn {
        if d.byte(p) == 10 {
            lineno += 1;
            y += 1;
            if y >= rows - 3 {
                while top < p && d.byte(top) != 10 {
                    top += 1;
                }
                top += 1;
                y -= 1
            }
        }
        p += 1;
    }
    let mut col = 0u16;
    let mut cursor_col = 0u16;
    p = start(d, posn);
    while p < posn {
        let n = unit(d, p);
        cursor_col += if n > 1 { 2 } else { 1 };
        p += n;
    }
    let left = (cursor_col / 70) * 70;
    p = top;
    y = 1;
    while p <= d.text_length && y < rows - 2 {
        let cursor = p == posn;
        let n = if p < d.text_length { unit(d, p) } else { 1 };
        let c = if p < d.text_length { d.byte(p) } else { 32 };
        if col >= left && col < left + 77 {
            screen(3, 1 + (col - left) as u16, y);
            screen(4, if cursor { 15 } else { 0 }, if cursor { 3 } else { 7 });
            if c == 13 || c == 10 || p == d.text_length {
                put(32)
            } else if c < 32 {
                put(32)
            } else {
                let mut j = 0;
                while j < n {
                    put(d.byte(p + j) as u16);
                    j += 1;
                }
            }
        }
        if c == 13 || c == 10 {
            y += 1;
            col = 0
        } else {
            col += if n > 1 { 2 } else { 1 }
        }
        p += n;
    }
    screen(4, 15, 1);
    screen(10, (rows - 1) << 8, 0x150);
    screen(3, 1, rows - 1);
    lang.message(Message::Bar);
    crate::install::number(lineno);
    screen(3, 1, rows - 2);
    screen(4, 0, 7);
    if let Some(msg) = status {
        lang.message(msg);
    }
}
fn prompt(lang: &Language, title: Message, buf: &mut [u8], query: bool) -> usize {
    let y = screen(5, 0, 0) - 2;
    screen(4, 0, 7);
    screen(10, y << 8, 0x150);
    screen(3, 1, y);
    lang.message(title);
    let mut n = 0;
    loop {
        let c = screen(15, 0, 0);
        if c == 27 {
            return 0;
        }
        if c == 13 {
            buf[n] = 0;
            return n;
        }
        if query && (c == 16 || c == 21) {
            let mut chars = [0u8; 4];
            let count = input::compose_with(c, &mut chars, lang);
            if count != 0 && n + count < buf.len() {
                let mut j = 0;
                while j < count {
                    buf[n + j] = chars[j];
                    j += 1;
                }
                n += count;
            }
            screen(4, 0, 7);
            screen(10, y << 8, 0x150);
            screen(3, 1, y);
            lang.message(title);
            let mut j = 0;
            while j < n {
                put(buf[j] as u16);
                j += 1;
            }
        } else if c == 8 && n > 0 {
            let mut i = 0;
            let mut last = 0;
            while i < n {
                last = i;
                i += if buf[i] < 128 {
                    1
                } else if i + 1 < n && buf[i + 1] >= 48 && buf[i + 1] <= 57 {
                    4
                } else {
                    2
                };
            }
            if n - last > 1 {
                text("\x08 \x08");
            }
            text("\x08 \x08");
            n = last;
        } else if c >= 32 && c < 127 && n + 1 < buf.len() {
            buf[n] = c as u8;
            n += 1;
            put(c)
        }
    }
}
fn find(d: &Document, q: &[u8], n: usize, start: usize) -> Option<usize> {
    if n == 0 || n > d.text_length {
        return None;
    }
    let mut checked = 0usize;
    let mut at = start;
    while checked <= d.text_length {
        if at > d.text_length - n {
            at = 0
        }
        let mut i = 0;
        while i < n && d.byte(at + i) == q[i] {
            i += 1
        }
        if i == n {
            return Some(at);
        }
        at += 1;
        checked += 1;
    }
    None
}
pub fn run() -> u16 {
    let mut lang = Language(if screen(7, 0, 0) == 7 {
        dosrs_realmode::language::ENGLISH
    } else {
        dosrs_realmode::language::selected()
    });
    let mut path = [0u8; 64];
    let mut i = 0;
    while i < 63 {
        let c = unsafe { arg(1, i as u16) };
        if c == 0 {
            break;
        }
        path[i] = c as u8;
        i += 1;
    }
    if i == 63 {
        lang.message(Message::Usage);
        text("\r\n");
        return 1;
    }
    let heap = unsafe { edit_heap() };
    let limit = dosrs_realmode::system::w(dosrs_realmode::PROGRAM, 2);
    if heap >= limit {
        lang.message(Message::Memory);
        text("\r\n");
        return 1;
    }
    let paras = limit - heap;
    let capacity = if paras > 4064 { 65024 } else { paras * 16 };
    let initial = if i == 0 {
        Ok(0)
    } else {
        load(&path, heap, capacity)
    };
    let Ok(n) = initial else {
        lang.message(Message::OpenError);
        crate::install::number(capacity as u32);
        text("\r\n");
        return 1;
    };
    let mut doc = Document::open(
        dos::Segment {
            base: heap,
            size: capacity,
        },
        n,
    );
    let mut pos = 0;
    let mut saved_generation = 0;
    let mut changes = history::History::new();
    let mut query = [0u8; 40];
    let mut query_len = 0;
    let mut quit = false;
    let mut status = None;
    screen(9, 0, 0);
    loop {
        let dirty = doc.generation != saved_generation;
        draw(&doc, &path, pos, status, dirty, &lang);
        let k = screen(15, 0, 0);
        status = None;
        if k == 17 {
            if !dirty || quit {
                break;
            }
            quit = true;
            status = Some(Message::Dirty);
            continue;
        }
        quit = false;
        match k {
            19 | 0x13c => {
                let mut dest = path;
                if k == 0x13c || path[0] == 0 {
                    if prompt(&lang, Message::SaveAs, &mut dest, false) == 0 {
                        continue;
                    }
                }
                if save(&dest, &mut doc) {
                    path = dest;
                    saved_generation = doc.generation;
                    status = Some(Message::Saved)
                } else {
                    status = Some(Message::Failed)
                }
            }
            26 | 25 => {
                if let Some(p) = changes.restore(&mut doc, k == 25) {
                    pos = p
                }
            }
            6 | 0x13d => {
                if k == 6 {
                    query_len = prompt(&lang, Message::Find, &mut query, true)
                }
                if let Some(p) = find(&doc, &query, query_len, pos + 1) {
                    pos = p;
                    status = Some(Message::Found)
                } else if query_len != 0 {
                    status = Some(Message::Missing)
                }
            }
            0x173 | 0x174 => pos = navigation::word(&doc, pos, k == 0x174),
            0x149 | 0x151 => {
                let mut rows = screen(5, 0, 0) - 3;
                while rows > 0 {
                    pos = vertical(&doc, pos, k == 0x151);
                    rows -= 1;
                }
            }
            0x13b => {
                lang.help();
            }

            0x13e => lang.choose(),
            0x14b => pos = previous(&doc, pos),
            0x14d => {
                if pos < doc.text_length {
                    pos += unit(&doc, pos)
                }
            }
            0x147 => pos = start(&doc, pos),
            0x14f => pos = end(&doc, pos),
            0x148 | 0x150 => pos = vertical(&doc, pos, k == 0x150),
            8 | 0x153 => {
                let from = if k == 8 { previous(&doc, pos) } else { pos };
                let n = if k == 8 {
                    pos - from
                } else if pos < doc.text_length {
                    unit(&doc, pos)
                } else {
                    0
                };
                if n > 0 && changes.replace(&mut doc, pos, from, n, b"", 0) {
                    pos = from;
                }
            }
            13 | 9 | 16 | 21 | 32..=126 => {
                let mut chars = [0u8; 4];
                let n = if k == 13 {
                    chars[0] = 13;
                    chars[1] = 10;
                    2
                } else if k == 16 || k == 21 {
                    input::compose_with(k, &mut chars, &lang)
                } else {
                    chars[0] = k as u8;
                    1
                };
                if n > 0 {
                    if doc.text_length + n <= doc.capacity
                        && changes.replace(&mut doc, pos, pos, 0, &chars, n)
                    {
                        pos += n;
                    } else {
                        status = Some(Message::Full)
                    }
                }
            }
            _ => (),
        }
    }
    screen(2, 0, 0);
    0
}
