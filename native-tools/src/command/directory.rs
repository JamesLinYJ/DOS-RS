use super::*;
struct Options {
    bare: bool,
    wide: bool,
    pause: bool,
    lower: bool,
    recursive: bool,
    require: u8,
    exclude: u8,
    order: [u8; 8],
    keys: usize,
}
impl Options {
    fn read(args: &[u8]) -> Result<(Self, Path), u16> {
        let mut o = Self {
            bare: false,
            wide: false,
            pause: false,
            lower: false,
            recursive: false,
            require: 0,
            exclude: 6,
            order: [0; 8],
            keys: 0,
        };
        let mut file = Path::empty();
        let mut i = 0;
        while i < args.len() {
            if parse::space(args[i]) {
                i += 1;
                continue;
            }
            if args[i] != b'/' {
                let start = i;
                while i < args.len() && !parse::space(args[i]) && args[i] != b'/' {
                    i += 1;
                }
                if file.n != 0 {
                    return Err(87);
                }
                file = Path::from(parse::part(args, start, i))?;
                continue;
            }
            i += 1;
            let enable = i >= args.len() || args[i] != b'-';
            if !enable {
                i += 1;
            }
            if i >= args.len() {
                return Err(87);
            }
            let key = upper(args[i]);
            i += 1;
            match key {
                b'B' => o.bare = enable,
                b'W' => o.wide = enable,
                b'P' => o.pause = enable,
                b'L' => o.lower = enable,
                b'S' => o.recursive = enable,
                b'A' | b'O' => {
                    if key == b'A' {
                        o.exclude = if enable { 0 } else { 6 };
                        o.require = 0;
                    } else {
                        o.keys = 0;
                    }
                    if i < args.len() && args[i] == b':' {
                        i += 1;
                    }
                    while i < args.len() && args[i] != b'/' && !parse::space(args[i]) {
                        let reverse = args[i] == b'-';
                        if reverse {
                            i += 1;
                        }
                        if i == args.len() {
                            return Err(87);
                        }
                        let c = upper(args[i]);
                        i += 1;
                        if key == b'A' {
                            let mask = match c {
                                b'D' => 16,
                                b'R' => 1,
                                b'H' => 2,
                                b'A' => 32,
                                b'S' => 4,
                                _ => return Err(87),
                            };
                            if reverse {
                                o.exclude |= mask;
                            } else {
                                o.require |= mask;
                            }
                        } else {
                            if o.keys == 8 || !matches!(c, b'N' | b'E' | b'G' | b'S' | b'D' | b'C')
                            {
                                return Err(87);
                            }
                            o.order[o.keys] = c | if reverse { 128 } else { 0 };
                            o.keys += 1;
                        }
                    }
                    if key == b'O' && o.keys == 0 && enable {
                        o.order[0] = b'G';
                        o.order[1] = b'N';
                        o.keys = 2;
                    }
                }
                b'C' => {
                    if i < args.len() && upper(args[i]) == b'H' {
                        i += 1;
                    }
                }
                _ => return Err(87),
            }
        }
        Ok((o, file))
    }
}
fn cmp(a: &[u8], b: &[u8]) -> i16 {
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return if a[i] < b[i] { -1 } else { 1 };
        }
        i += 1;
    }
    if a.len() == b.len() {
        0
    } else if a.len() < b.len() {
        -1
    } else {
        1
    }
}
fn ext(name: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < name.len() && name[i] != b'.' {
        i += 1;
    }
    if i < name.len() {
        i += 1;
    }
    parse::rest(name, i)
}
fn rank(a: u32, b: u32) -> i16 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}
fn order(a: &Search, b: &Search, o: &Options) -> i16 {
    let mut i = 0;
    while i < o.keys {
        let key = o.order[i];
        let v = match key & 127 {
            b'G' => rank((b.attr() & 16) as u32, (a.attr() & 16) as u32),
            b'E' => cmp(ext(a.name()), ext(b.name())),
            b'S' | b'C' => rank(a.size(), b.size()),
            b'D' => rank(
                (u32::from(a.word(24)) << 16) | u32::from(a.word(22)),
                (u32::from(b.word(24)) << 16) | u32::from(b.word(22)),
            ),
            _ => cmp(a.name(), b.name()),
        };
        if v != 0 {
            return if key & 128 == 0 {
                v
            } else if v < 0 {
                1
            } else {
                -1
            };
        }
        i += 1;
    }
    cmp(a.name(), b.name())
}
fn print_name(b: &[u8], lower: bool) {
    if !lower {
        dos::out(b);
        return;
    }
    let mut buf = [0; 128];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        buf[i] = if c >= b'A' && c <= b'Z' { c + 32 } else { c };
        i += 1;
    }
    dos::out(parse::head(&buf, i));
}
fn format_entry(e: &Search, parent: &Path, o: &Options, column: &mut usize) -> Result<(), u16> {
    let name = e.name();
    if o.bare {
        if o.recursive {
            let mut p = *parent;
            p.join(name)?;
            print_name(p.bytes(), o.lower);
        } else {
            print_name(name, o.lower);
        }
        newline();
        return Ok(());
    }
    if o.wide {
        let dir = e.attr() & 16 != 0;
        if dir {
            byte(b'[');
        }
        print_name(name, o.lower);
        if dir {
            byte(b']');
        }
        let mut i = name.len() + if dir { 2 } else { 0 };
        *column += 1;
        if *column == 5 {
            newline();
            *column = 0;
        } else {
            while i < 16 {
                byte(b' ');
                i += 1;
            }
        }
        return Ok(());
    }
    let mut i = 0;
    while i < name.len() && name[i] != b'.' {
        i += 1;
    }
    let root = if name[0] == b'.' {
        name
    } else {
        parse::head(name, i)
    };
    print_name(root, o.lower);
    let mut n = root.len();
    while n < 9 {
        byte(b' ');
        n += 1;
    }
    let suffix = if name[0] == b'.' { b"" } else { ext(name) };
    print_name(suffix, o.lower);
    n = suffix.len();
    while n < 4 {
        byte(b' ');
        n += 1;
    }
    if e.attr() & 16 != 0 {
        dos::out(b"  <DIR>   ");
    } else {
        number(e.size(), 10);
    }
    let date = e.word(24);
    let time = e.word(22);
    dos::out(b"  ");
    two((date >> 5) & 15);
    byte(b'-');
    two(date & 31);
    byte(b'-');
    two((1980 + (date >> 9)) % 100);
    dos::out(b"  ");
    let h = time >> 11;
    let hr = h % 12;
    number(u32::from(if hr == 0 { 12 } else { hr }), 2);
    byte(b':');
    two((time >> 5) & 63);
    byte(if h >= 12 { b'p' } else { b'a' });
    newline();
    Ok(())
}
struct Totals {
    files: u32,
    size: u32,
    lines: u16,
}
fn listed(s: &Search, o: &Options) -> bool {
    s.attr() & o.exclude == 0
        && s.attr() & o.require == o.require
        && s.attr() & 8 == 0
        && (!o.bare || !equal(s.name(), b".") && !equal(s.name(), b".."))
}
fn show(
    s: &Search,
    parent: &Path,
    o: &Options,
    t: &mut Totals,
    column: &mut usize,
) -> Result<(), u16> {
    format_entry(s, parent, o, column)?;
    t.files += 1;
    if s.attr() & 16 == 0 {
        t.size = if t.size > u32::MAX - s.size() {
            u32::MAX
        } else {
            t.size + s.size()
        };
    }
    if *column == 0 {
        t.lines += 1;
    }
    if o.pause && t.lines >= 23 {
        dos::out(b"Press any key to continue . . .");
        let _ = dos::service(0x0800, 0, 0, 0);
        newline();
        t.lines = 0;
    }
    Ok(())
}
fn list(parent: &mut Path, leaf: &Path, o: &Options, t: &mut Totals) -> Result<(), u16> {
    let mut p = *parent;
    p.join(leaf.bytes())?;
    let mut col = 0;
    let mut s = Search::new();
    let mut any = false;
    if !o.bare {
        dos::out(b"\r\n Directory of ");
        print_name(parent.bytes(), o.lower);
        dos::out(b"\r\n\r\n");
    }
    if o.keys == 0 {
        while s.next(&p, 0x37)? {
            if listed(&s, o) {
                show(&s, parent, o, t, &mut col)?;
            }
        }
    } else {
        let mut last = Search::new();
        loop {
            let mut best = Search::new();
            let mut seen = false;
            s.started = false;
            while s.next(&p, 0x37)? {
                if listed(&s, o)
                    && (!any || order(&s, &last, o) > 0)
                    && (!seen || order(&s, &best, o) < 0)
                {
                    best.data = s.data;
                    seen = true;
                }
            }
            if !seen {
                break;
            }
            show(&best, parent, o, t, &mut col)?;
            last.data = best.data;
            any = true;
        }
    }
    if col != 0 {
        newline();
    }
    if o.recursive {
        p = *parent;
        p.join(b"*.*")?;
        s.started = false;
        while s.next(&p, 0x37)? {
            if s.attr() & 16 != 0
                && s.attr() & o.exclude == 0
                && !equal(s.name(), b".")
                && !equal(s.name(), b"..")
            {
                let n = parent.n;
                parent.join(s.name())?;
                if parent.n <= 64 {
                    list(parent, leaf, o, t)?;
                }
                parent.trim(n);
            }
        }
    }
    Ok(())
}
pub(super) fn dir(args: &[u8]) -> Result<u16, u16> {
    let (o, mut pattern) = Options::read(args)?;
    if pattern.n == 0
        || matches!(pattern.buf[pattern.n - 1], b':' | b'\\')
        || pattern.attribute().is_ok_and(|a| a & 16 != 0)
    {
        pattern.join(b"*.*")?;
    }
    let head = pattern.component();
    let leaf = Path::from(parse::rest(pattern.bytes(), head))?;
    pattern.trim(head);
    if head == 0 || pattern.buf[head - 1] == b':' {
        pattern.push(b".")?;
    }
    let mut parent = pattern.true_name()?;
    if parent.n > 3 && parent.buf[parent.n - 1] == b'\\' {
        parent.trim(parent.n - 1);
    }
    let drive = if parent.n >= 2 && parent.buf[1] == b':' {
        u16::from(upper(parent.buf[0]) - b'A')
    } else {
        dos::drive()
    };
    if !o.bare {
        vol(&[b'A' + drive as u8, b':'])?;
    }
    let mut t = Totals {
        files: 0,
        size: 0,
        lines: 0,
    };
    list(&mut parent, &leaf, &o, &mut t)?;
    if t.files == 0 {
        return Err(2);
    }
    if !o.bare {
        number(t.files, 9);
        dos::out(b" file(s) ");
        number(t.size, 12);
        dos::out(b" bytes\r\n");
        let mut r = [0; 12];
        r[0] = 0x3600;
        r[3] = drive + 1;
        if dos::call(&mut r) && r[0] != 0xffff {
            number(u32::from(r[0]) * u32::from(r[1]) * u32::from(r[2]), 12);
            dos::out(b" bytes free\r\n");
        }
    }
    Ok(0)
}
