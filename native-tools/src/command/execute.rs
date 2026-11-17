use super::{Shell, batch, builtins, dos, parse, prompt};
use batch::Frame;

pub fn syntax(sh: &mut Shell) {
    dos::err(b"Syntax error\r\n");
    sh.status = 1;
}
fn known(name: &[u8], ext: &[u8]) -> bool {
    name.len() > ext.len() && parse::equal(parse::rest(name, name.len() - ext.len()), ext)
}
fn locate(name: &[u8], dirs: &[u8], out: &mut [u8]) -> usize {
    let mut explicit = false;
    let mut ext = false;
    let mut i = 0;
    while i < name.len() {
        let c = name[i];
        if c == b':' || c == b'\\' {
            explicit = true;
            ext = false;
        } else if c == b'.' {
            ext = true;
        }
        i += 1;
    }
    let mut from = 0;
    let mut cwd = true;
    loop {
        let mut n = 0;
        if !cwd {
            let end = from + parse::split_at(parse::rest(&dirs, from), b';');
            if !parse::push(out, &mut n, parse::unquote(parse::part(&dirs, from, end))) {
                return 0;
            }
            if n > 0
                && out[n - 1] != b':'
                && out[n - 1] != b'\\'
                && !parse::push(out, &mut n, b"\\")
            {
                return 0;
            }
            from = if end == dirs.len() { end } else { end + 1 };
        }
        if parse::push(out, &mut n, name) {
            if ext {
                if dos::exists(parse::head(&out, n)) {
                    return n;
                }
            } else {
                let start = n;
                let mut k = 0;
                while k < 3 {
                    n = start;
                    let e: &[u8] = match k {
                        0 => b".COM",
                        1 => b".EXE",
                        _ => b".BAT",
                    };
                    if parse::push(out, &mut n, e) && dos::exists(parse::head(&out, n)) {
                        return n;
                    }
                    k += 1;
                }
            }
        }
        if explicit || from >= dirs.len() {
            break;
        }
        cwd = false;
    }
    0
}
fn external(sh: &mut Shell, f: &mut Frame, name: &[u8], args: &[u8], called: bool) {
    let mut dirs = [0; 256];
    let len = sh.env.get(b"PATH", &mut dirs).unwrap_or(0);
    let mut path = [0; 128];
    let n = locate(name, parse::head(&dirs, len), &mut path);
    if n == 0 {
        dos::err(b"Bad command or file name\r\n");
        sh.status = 1;
        return;
    }
    if known(parse::head(&path, n), b".BAT") {
        if f.active() && !called {
            f.done = true;
        }
        sh.status = batch::run(sh, parse::head(&path, n), args);
    } else {
        sh.status = match dos::exec(parse::head(&path, n), args) {
            Ok(code) => code,
            Err(_) => {
                dos::err(b"Unable to execute program\r\n");
                1
            }
        };
    }
}
fn variable(sh: &mut Shell, name: &[u8], args: &[u8]) {
    if parse::equal(name, b"SET") {
        if args.is_empty() {
            sh.env.list();
            return;
        }
        let pos = parse::split_at(args, b'=');
        if pos == 0 || pos >= args.len() {
            syntax(sh);
            return;
        }
        if !sh
            .env
            .assign(parse::head(&args, pos), parse::rest(&args, pos + 1))
        {
            dos::err(b"Out of environment space\r\n");
            sh.status = 1;
        }
    } else if parse::equal(name, b"PATH") && args.is_empty() {
        let mut buf = [0; 256];
        if let Some(n) = sh.env.get(b"PATH", &mut buf) {
            dos::out(b"PATH=");
            dos::out(parse::head(&buf, n));
            dos::out(b"\r\n");
        } else {
            dos::out(b"No Path\r\n");
        }
    } else {
        let mut s = args;
        if !s.is_empty() && s[0] == b'=' {
            s = parse::rest(&s, 1);
        }
        if parse::equal(name, b"PATH") && parse::equal(s, b";") {
            s = b"";
        }
        if !sh.env.assign(name, s) {
            dos::err(b"Out of environment space\r\n");
            sh.status = 1;
        }
    }
}
fn dispatch(sh: &mut Shell, f: &mut Frame, cmd: &[u8], called: bool) {
    let (name, args) = parse::command(cmd);
    if name.is_empty() {
        return;
    }
    if parse::equal(parse::trim(args), b"/?") && builtins::help(name) {
        sh.status = 0;
        return;
    }
    if name.len() == 2 && name[1] == b':' {
        let d = parse::upper(name[0]);
        if d >= b'A' && d <= b'Z' {
            let _ = dos::service(0x0e00, 0, 0, u16::from(d - b'A'));
            if dos::drive() != u16::from(d - b'A') {
                dos::err(b"Invalid drive specification\r\n");
                sh.status = 1;
            }
        }
        return;
    }
    if parse::equal(name, b"REM") {
        return;
    }
    if parse::equal(name, b"ECHO") {
        if parse::equal(parse::trim(args), b"OFF") {
            sh.echo = false;
        } else if parse::equal(parse::trim(args), b"ON") {
            sh.echo = true;
        } else if args.is_empty() {
            dos::out(if sh.echo {
                b"ECHO is on\r\n"
            } else {
                b"ECHO is off\r\n"
            });
        } else {
            if args[0] == b'.' {
                dos::out(parse::rest(&args, 1));
            } else {
                dos::out(args);
            }
            dos::out(b"\r\n");
        }
        return;
    }
    if parse::equal(name, b"PAUSE") {
        dos::out(b"Press any key to continue . . .");
        if dos::console(0) {
            let _ = dos_tools::key();
        } else {
            let mut one = [0; 1];
            let _ = dos::read(0, &mut one);
        }
        dos::out(b"\r\n");
        return;
    }
    if parse::equal(name, b"EXIT") {
        if !sh.permanent {
            sh.exit = true;
        }
        return;
    }
    if parse::equal(name, b"PROMPT") || parse::equal(name, b"PATH") || parse::equal(name, b"SET") {
        variable(sh, name, args);
        return;
    }
    if batch::builtin(sh, f, name, args) {
        return;
    }
    if parse::equal(name, b"LOADHIGH") || parse::equal(name, b"LH") {
        let mut rest = args;
        while !rest.is_empty() && rest[0] == b'/' {
            rest = parse::word(rest).1;
        }
        let (program, tail) = parse::word(rest);
        if program.is_empty() {
            syntax(sh);
            return;
        }
        let strategy = dos::service(0x5800, 0, 0, 0).unwrap_or(0);
        let linked = dos::service(0x5802, 0, 0, 0).unwrap_or(0);
        let _ = dos::service(0x5803, 1, 0, 0);
        let _ = dos::service(0x5801, 0x80 | (strategy & 3), 0, 0);
        external(sh, f, program, tail, called);
        let _ = dos::service(0x5801, strategy, 0, 0);
        let _ = dos::service(0x5803, linked & 255, 0, 0);
        return;
    }
    if let Some(code) = builtins::run(name, args, f.active()) {
        sh.status = code;
        return;
    }
    external(sh, f, name, args, called)
}
struct Redirect {
    saved: [u16; 2],
}
impl Redirect {
    fn new() -> Self {
        Self { saved: [0xffff; 2] }
    }
    fn attach(&mut self, unit: u16, fd: u16) -> bool {
        let target = unit as usize;
        if self.saved[target] == 0xffff {
            let Ok(backup) = dos::service(0x4500, unit, 0, 0) else {
                dos::close(fd);
                return false;
            };
            self.saved[target] = backup;
        }
        let ok = dos::service(0x4600, fd, unit, 0).is_ok();
        dos::close(fd);
        ok
    }
    fn restore(&mut self) {
        let mut i = 0;
        while i < 2 {
            let fd = self.saved[i];
            if fd != 0xffff {
                let _ = dos::service(0x4600, fd, i as u16, 0);
                dos::close(fd);
            }
            i += 1;
        }
    }
}
fn redirected(sh: &mut Shell, f: &mut Frame, data: &[u8], called: bool, io: &mut Redirect) {
    let mut buf = [0; parse::LIMIT];
    let mut n = 0;
    let mut i = 0;
    let mut quoted = false;
    while i < data.len() {
        let c = data[i];
        i += 1;
        if c == b'"' {
            quoted = !quoted;
        }
        if !quoted && (c == b'<' || c == b'>') {
            let append = c == b'>' && i < data.len() && data[i] == b'>';
            if append {
                i += 1;
            }
            while i < data.len() && parse::space(data[i]) {
                i += 1;
            }
            let start = i;
            let mut q = false;
            while i < data.len() {
                let b = data[i];
                if b == b'"' {
                    q = !q;
                }
                if !q && (parse::space(b) || b == b'<' || b == b'>') {
                    break;
                }
                i += 1;
            }
            if start == i {
                syntax(sh);
                return;
            }
            let name = parse::part(&data, start, i);
            let opened = if c == b'<' {
                dos::open(name, 0)
            } else if append {
                match dos::open(name, 1) {
                    Ok(fd) => {
                        if dos::seek(fd, 2, 0).is_err() {
                            dos::close(fd);
                            Err(5)
                        } else {
                            Ok(fd)
                        }
                    }
                    Err(2) => dos::create(name),
                    Err(e) => Err(e),
                }
            } else {
                dos::create(name)
            };
            if let Ok(fd) = opened {
                if io.attach(if c == b'<' { 0 } else { 1 }, fd) {
                    continue;
                }
            }
            dos::err(b"Redirection failed\r\n");
            sh.status = 1;
            return;
        }
        buf[n] = c;
        n += 1;
    }
    dispatch(sh, f, parse::head(&buf, n), called);
}
fn temp(sh: &mut Shell, path: &mut [u8]) -> Option<(usize, u16)> {
    let mut n = sh
        .env
        .get(b"TEMP", path)
        .or_else(|| sh.env.get(b"TMP", path))
        .unwrap_or(0);
    if n > 0 && path[n - 1] != b':' && path[n - 1] != b'\\' && !parse::push(path, &mut n, b"\\") {
        return None;
    }
    let start = n;
    if !parse::push(path, &mut n, b"P0000000.TMP") {
        return None;
    }
    let hex = b"0123456789ABCDEF";
    let owner = dos::psp();
    let mut attempt = 0;
    while attempt < 64 {
        let mut k = 0;
        while k < 3 {
            path[start + 1 + k] = hex[((owner >> (4 * k)) & 15) as usize];
            k += 1;
        }
        k = 0;
        while k < 4 {
            path[start + 4 + k] = hex[((sh.sequence >> (4 * k)) & 15) as usize];
            k += 1;
        }
        sh.sequence = sh.sequence.wrapping_add(1);
        match dos::path_call(0x5b00, parse::head(&path, n), 0) {
            Ok(fd) => return Some((n, fd)),
            Err(80) => attempt += 1,
            Err(_) => return None,
        }
    }
    None
}
pub fn line(sh: &mut Shell, f: &mut Frame, data: &[u8], called: bool) {
    if data.len() > parse::LIMIT {
        syntax(sh);
        return;
    }
    let mut data = parse::left(data);
    if !data.is_empty() && data[0] == b'@' {
        data = parse::rest(&data, 1);
    }
    if !parse::contains(data, b'|') && !parse::contains(data, b'<') && !parse::contains(data, b'>')
    {
        dispatch(sh, f, data, called);
        return;
    }
    let mut prior = [0; 128];
    let mut prior_len = 0;
    loop {
        let stop = parse::split_at(data, b'|');
        let more = stop < data.len();
        let stage = parse::trim(parse::head(&data, stop));
        if stage.is_empty() {
            syntax(sh);
            if prior_len > 0 {
                dos::remove(parse::head(&prior, prior_len));
            }
            return;
        }
        let mut io = Redirect::new();
        let mut next = [0; 128];
        let mut next_len = 0;
        let mut ok = true;
        if prior_len > 0 {
            ok = match dos::open(parse::head(&prior, prior_len), 0) {
                Ok(fd) => io.attach(0, fd),
                Err(_) => false,
            };
        }
        if ok && more {
            if let Some((len, fd)) = temp(sh, &mut next) {
                next_len = len;
                ok = io.attach(1, fd);
            } else {
                ok = false;
            }
        }
        if ok {
            redirected(sh, f, stage, called, &mut io);
        } else {
            dos::err(b"Unable to create pipe\r\n");
            sh.status = 1;
        }
        io.restore();
        if prior_len > 0 {
            dos::remove(parse::head(&prior, prior_len));
        }
        if !more || !ok || sh.exit {
            if next_len > 0 {
                dos::remove(parse::head(&next, next_len));
            }
            return;
        }
        prior_len = 0;
        parse::push(&mut prior, &mut prior_len, parse::head(&next, next_len));
        data = parse::rest(&data, stop + 1);
    }
}
pub fn interactive(sh: &mut Shell) {
    let mut frame = Frame::new(0xffff, b"", b"");
    let mut line_buf = [0; parse::LIMIT];
    let mut ends = [0; parse::LIMIT];
    let tty = dos::console(0);
    loop {
        if sh.exit {
            return;
        }
        if tty {
            prompt::display(&sh.env);
        }
        let mut n = 0;
        let mut chars = 0;
        loop {
            let c = if tty {
                dos_tools::key()
            } else {
                let mut b = [0; 1];
                if dos::read(0, &mut b).unwrap_or(0) == 0 {
                    if n == 0 {
                        return;
                    }
                    break;
                }
                u16::from(b[0])
            };
            if c == 13 || c == 10 {
                if tty {
                    dos::out(b"\r\n");
                }
                break;
            }
            if c == 26 && !tty {
                return;
            }
            if c == 8 && chars > 0 {
                chars -= 1;
                let prior = ends[chars] as usize;
                if tty {
                    if n - prior > 1 {
                        dos::out(b"\r\n");
                        prompt::display(&sh.env);
                        dos::out(parse::head(&line_buf, prior));
                    } else {
                        dos::out(b"\x08 \x08");
                    }
                }
                n = prior;
            } else if tty && (c == 16 || c == 21) {
                let mut buf = [0u8; 4];
                let size = dos_tools::input::compose(c, &mut buf);
                if size != 0 && n + size <= parse::LIMIT {
                    ends[chars] = n as u8;
                    chars += 1;
                    parse::push(&mut line_buf, &mut n, parse::head(&buf, size));
                }
                dos::out(b"\r\n");
                prompt::display(&sh.env);
                dos::out(parse::head(&line_buf, n));
            } else if c >= 32 && c <= 255 && n < parse::LIMIT {
                ends[chars] = n as u8;
                chars += 1;
                line_buf[n] = c as u8;
                n += 1;
                if tty {
                    dos::out(parse::part(&line_buf, n - 1, n));
                }
            }
        }
        let mut expanded = [0; parse::LIMIT];
        if let Some(n) = batch::expand(sh, &frame, parse::head(&line_buf, n), &mut expanded) {
            line(sh, &mut frame, parse::head(&expanded, n), false);
        } else {
            syntax(sh);
        }
    }
}
