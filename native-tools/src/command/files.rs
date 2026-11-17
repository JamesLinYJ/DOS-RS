use super::*;

fn transfer(fd: u16, out: u16, text: bool) -> Result<(), u16> {
    let mut buf = [0u8; 1024];
    loop {
        let got = dos::read(fd, &mut buf)? as usize;
        if got == 0 {
            return Ok(());
        }
        let mut n = 0;
        while n < got && (!text || buf[n] != 26) {
            n += 1;
        }
        let mut p = 0;
        while p < n {
            let written = dos::write(out, parse::part(&buf, p, n))? as usize;
            if written == 0 {
                return Err(112);
            }
            p += written;
        }
        if n != got {
            return Ok(());
        }
    }
}
pub(super) fn display(args: &[u8]) -> Result<u16, u16> {
    let (arg, rest) = parse::word(args);
    if arg.is_empty() || !rest.is_empty() {
        return Err(87);
    }
    let fd = dos::open(arg, 0)?;
    let read = transfer(fd, 1, true);
    dos::close(fd);
    read?;
    Ok(0)
}

// Substitute wildcard characters against DOS's space-padded 8.3 fields.
fn replacement(source: &[u8], mask: &[u8]) -> Result<Path, u16> {
    if !wildcard(mask) {
        return Path::from(mask);
    }
    let mut dst = Path::empty();
    let mut i = 0;
    let mut j = 0;
    let mut field = 0;
    while field < 2 {
        let mut matched = 0;
        let width = if field == 0 { 8 } else { 3 };
        let start = dst.n;
        while j < mask.len() && mask[j] != b'.' {
            let c = mask[j];
            j += 1;
            if c == b'*' {
                while i < source.len() && source[i] != b'.' && matched < width {
                    dst.push(&[source[i]])?;
                    i += 1;
                    matched += 1;
                }
            } else {
                if matched >= width {
                    return Err(87);
                }
                if c == b'?' {
                    if i < source.len() && source[i] != b'.' {
                        dst.push(&[source[i]])?;
                    }
                } else {
                    dst.push(&[c])?;
                }
                if i < source.len() && source[i] != b'.' {
                    i += 1;
                }
                matched += 1;
            }
        }
        if field == 1 && dst.n == start {
            dst.trim(start - 1);
        }
        if field == 0 {
            while i < source.len() && source[i] != b'.' {
                i += 1;
            }
            if i < source.len() {
                i += 1;
            }
            if j == mask.len() {
                break;
            }
            j += 1;
            dst.push(b".")?;
        }
        field += 1;
    }
    Ok(dst)
}
fn prompt(question: &[u8]) -> bool {
    dos::out(question);
    let mut answer = [0; 1];
    let ok = matches!(dos::read(0, &mut answer), Ok(1)) && upper(answer[0]) == b'Y';
    newline();
    ok
}
fn lookup_pattern(path: &mut Path) -> Result<(), u16> {
    if !wildcard(path.bytes()) && path.attribute().is_ok_and(|a| a & 16 != 0) {
        path.join(b"*.*")?;
    }
    Ok(())
}
pub(super) fn delete(args: &[u8]) -> Result<u16, u16> {
    let (first, left) = parse::word(args);
    let (second, rest) = parse::word(left);
    let ask = equal(first, b"/P") || equal(second, b"/P");
    let path = if equal(first, b"/P") { second } else { first };
    if path.is_empty() || !rest.is_empty() || (!second.is_empty() && !ask) {
        return Err(87);
    }
    let mut pattern = Path::from(path)?;
    lookup_pattern(&mut pattern)?;
    let head = pattern.component();
    let tail = parse::rest(pattern.bytes(), head);
    if !ask
        && (equal(tail, b"*.*") || equal(tail, b"*"))
        && !prompt(b"All files in directory will be deleted!\r\nAre you sure (Y/N)? ")
    {
        return Ok(0);
    }
    let mut found = Search::new();
    let mut any = false;
    while found.next(&pattern, 0)? {
        if found.attr() & 16 != 0 {
            continue;
        }
        let mut p = pattern;
        p.trim(head);
        p.push(found.name())?;
        any = true;
        if ask {
            dos::out(p.bytes());
            if !prompt(b", Delete (Y/N)? ") {
                continue;
            }
        }
        dos::path_call(0x4100, p.bytes(), 0)?;
    }
    if any { Ok(0) } else { Err(2) }
}
pub(super) fn rename(args: &[u8]) -> Result<u16, u16> {
    let (from, left) = parse::word(args);
    let (to, rest) = parse::word(left);
    if from.is_empty() || to.is_empty() || !rest.is_empty() {
        return Err(87);
    }
    let mask = Path::from(to)?;
    if mask.component() != 0 {
        return Err(87);
    }
    let pattern = Path::from(from)?;
    let head = pattern.component();
    let mut s = Search::new();
    let mut any = false;
    while s.next(&pattern, 0x10)? {
        let name = s.name();
        if equal(name, b".") || equal(name, b"..") {
            continue;
        }
        let mut src = pattern;
        src.trim(head);
        let mut dst = src;
        src.push(name)?;
        dst.push(replacement(name, mask.bytes())?.bytes())?;
        let mut r = [0; 12];
        r[0] = 0x5600;
        r[3] = dos::ptr(src.buf.as_ptr());
        r[5] = dos::ptr(dst.buf.as_ptr());
        r[6] = dos::ds();
        if !dos::call(&mut r) {
            return Err(r[0]);
        }
        any = true;
    }
    if any { Ok(0) } else { Err(2) }
}
struct CopyArgs<'a> {
    from: [&'a [u8]; 16],
    mode: [u8; 16],
    count: usize,
    out: &'a [u8],
    dest_mode: u8,
    overwrite: i8,
    verify: bool,
}
fn copy_args(args: &[u8]) -> Result<CopyArgs<'_>, u16> {
    let mut a = CopyArgs {
        from: [b""; 16],
        mode: [0; 16],
        count: 0,
        out: b"",
        dest_mode: 0,
        overwrite: 0,
        verify: false,
    };
    let mut i = 0;
    let mut plus = false;
    let mut mode = 0;
    while i < args.len() {
        while i < args.len() && parse::space(args[i]) {
            i += 1;
        }
        if i == args.len() {
            break;
        }
        if args[i] == b'+' {
            if a.count == 0 || plus || !a.out.is_empty() {
                return Err(87);
            }
            plus = true;
            i += 1;
            continue;
        }
        if args[i] == b'/' {
            i += 1;
            let start = i;
            while i < args.len() && !parse::space(args[i]) && !matches!(args[i], b'/' | b'+') {
                i += 1;
            }
            let opt = parse::part(args, start, i);
            if equal(opt, b"A") || equal(opt, b"B") {
                mode = if equal(opt, b"A") { 1 } else { 2 };
                if !a.out.is_empty() {
                    a.dest_mode = mode;
                } else if a.count != 0 {
                    a.mode[a.count - 1] = mode;
                }
            } else if equal(opt, b"V") {
                a.verify = true;
            } else if equal(opt, b"Y") {
                a.overwrite = 1;
            } else if equal(opt, b"-Y") {
                a.overwrite = -1;
            } else {
                return Err(87);
            }
            continue;
        }
        let start = i;
        while i < args.len() && !parse::space(args[i]) && !matches!(args[i], b'+' | b'/') {
            i += 1;
        }
        let piece = parse::part(args, start, i);
        if a.count == 0 || plus {
            if a.count == a.from.len() {
                return Err(87);
            }
            a.from[a.count] = piece;
            a.mode[a.count] = mode;
            a.count += 1;
            plus = false;
        } else if a.out.is_empty() {
            a.out = piece;
            a.dest_mode = mode;
        } else {
            return Err(87);
        }
    }
    if a.count == 0 || plus { Err(87) } else { Ok(a) }
}
fn same(a: &Path, b: &Path) -> bool {
    if equal(a.bytes(), b.bytes()) {
        return true;
    }
    match (a.true_name(), b.true_name()) {
        (Ok(x), Ok(y)) => equal(x.bytes(), y.bytes()),
        _ => false,
    }
}
fn check_overwrite(path: &Path, ask: bool) -> bool {
    if !ask || path.attribute().is_err() {
        return true;
    }
    dos::out(b"Overwrite ");
    dos::out(path.bytes());
    prompt(b" (Yes/No)? ")
}
fn copy_content(src: &Path, out: u16, text: bool, stamp: bool) -> Result<(), u16> {
    let fd = dos::open(src.bytes(), 0)?;
    let v = transfer(fd, out, text);
    if stamp && v.is_ok() {
        let mut t = [0; 12];
        t[0] = 0x5700;
        t[1] = fd;
        if dos::call(&mut t) {
            t[0] = 0x5701;
            t[1] = out;
            dos::call(&mut t);
        }
    }
    dos::close(fd);
    v
}
fn end_text(fd: u16) -> Result<(), u16> {
    if dos::write(fd, b"\x1a")? == 1 {
        Ok(())
    } else {
        Err(112)
    }
}
struct Output {
    fd: u16,
    name: Path,
    appended: bool,
    count: u32,
}
impl Output {
    fn finish(&mut self, text: bool) -> Result<(), u16> {
        if self.fd == 0xffff {
            return Ok(());
        }
        if text {
            end_text(self.fd)?;
        }
        if !dos::close(self.fd) {
            return Err(5);
        }
        self.fd = 0xffff;
        self.count += 1;
        Ok(())
    }
    fn discard(&mut self) {
        if self.fd != 0xffff {
            dos::close(self.fd);
            if !self.appended {
                dos::remove(self.name.bytes());
            }
            self.fd = 0xffff;
        }
    }
}
fn copies(a: &CopyArgs, batch: bool, out: &mut Output) -> Result<(), u16> {
    let first = Path::from(a.from[0])?;
    let mut target = if a.out.is_empty() && a.count > 1 {
        first
    } else {
        Path::from(a.out)?
    };
    let is_dir = target.n == 0
        || matches!(target.buf[target.n - 1], b':' | b'\\')
        || target.attribute().is_ok_and(|v| v & 16 != 0);
    let join = a.count > 1 || wildcard(first.bytes()) && !is_dir && !wildcard(target.bytes());
    if join && is_dir {
        target.join(parse::rest(first.bytes(), first.component()))?;
    }
    let ask = a.overwrite < 0 || (!batch && a.overwrite == 0);
    for i in 0..a.count {
        let spec = Path::from(a.from[i])?;
        let mut search = Search::new();
        let mut found = false;
        while search.next(&spec, 0)? {
            if search.attr() & 16 != 0 {
                continue;
            }
            found = true;
            let mut source = spec;
            source.trim(source.component());
            source.push(search.name())?;
            let mut dest = target;
            if !join && is_dir {
                dest.join(search.name())?;
            } else if !join && wildcard(dest.bytes()) {
                let p = dest.component();
                let name = replacement(search.name(), parse::rest(dest.bytes(), p))?;
                dest.trim(p);
                dest.push(name.bytes())?;
            }
            if same(&source, &dest) {
                if join && i == 0 && out.fd == 0xffff {
                    out.fd = dos::open(dest.bytes(), 2)?;
                    out.appended = true;
                    dos::seek(out.fd, 2, 0)?;
                    continue;
                }
                return Err(5);
            }
            if out.fd == 0xffff {
                if !check_overwrite(&dest, ask) {
                    if join {
                        return Ok(());
                    }
                    continue;
                }
                out.fd = dos::create(dest.bytes())?;
                out.name = dest;
                out.appended = false;
            }
            if join {
                dos::out(source.bytes());
                newline();
            }
            copy_content(
                &source,
                out.fd,
                a.mode[i] == 1 || (a.mode[i] == 0 && join),
                !join,
            )?;
            if !join {
                out.finish(a.dest_mode == 1)?;
            }
        }
        if !found {
            return Err(2);
        }
    }
    out.finish(a.dest_mode == 1 || join && a.dest_mode != 2)
}
pub(super) fn copy(args: &[u8], batch: bool) -> Result<u16, u16> {
    let parsed = copy_args(args)?;
    let old = if parsed.verify {
        let flag = dos::service(0x5400, 0, 0, 0)?;
        dos::service(0x2e01, 0, 0, 0)?;
        Some(flag & 1)
    } else {
        None
    };
    let mut out = Output {
        fd: 0xffff,
        name: Path::empty(),
        appended: false,
        count: 0,
    };
    let result = copies(&parsed, batch, &mut out);
    if result.is_err() {
        out.discard();
    }
    if let Some(flag) = old {
        let _ = dos::service(0x2e00 | flag, 0, 0, 0);
    }
    result?;
    number(out.count, 9);
    dos::out(b" file(s) copied\r\n");
    Ok(0)
}
