//! Batch files are streamed through a private DOS descriptor for each CALL frame.
use super::{Shell, dos, execute, parse, prompt};
pub struct Frame<'a> {
    fd: u16,
    name: &'a [u8],
    args: &'a [u8],
    pub shift: u16,
    pub done: bool,
    cache: [u8; 128],
    cursor: usize,
    count: usize,
}
impl<'a> Frame<'a> {
    pub fn new(fd: u16, name: &'a [u8], args: &'a [u8]) -> Self {
        Self {
            fd,
            name,
            args,
            shift: 0,
            done: false,
            cache: [0; 128],
            cursor: 0,
            count: 0,
        }
    }
    pub fn active(&self) -> bool {
        self.fd != 0xffff
    }
    fn argument(&self, index: u16) -> &[u8] {
        let mut k = index + self.shift;
        if k == 0 {
            return self.name;
        }
        let mut data = self.args;
        while k > 1 && !data.is_empty() {
            data = parse::word(data).1;
            k -= 1;
        }
        parse::word(data).0
    }
    fn line(&mut self, out: &mut [u8]) -> Option<usize> {
        let mut n = 0;
        loop {
            if self.cursor >= self.count {
                self.count = usize::from(dos::read(self.fd, &mut self.cache).unwrap_or(0));
                self.cursor = 0;
                if self.count == 0 {
                    return if n > 0 { Some(n) } else { None };
                }
            }
            let c = self.cache[self.cursor];
            self.cursor += 1;
            if c == 26 {
                self.done = true;
                return if n > 0 { Some(n) } else { None };
            }
            if c == 10 {
                return Some(n);
            }
            if c != 13 && n < out.len() {
                out[n] = c;
                n += 1;
            }
        }
    }
    fn jump(&mut self, label: &[u8]) -> bool {
        let mut target = parse::word(label).0;
        if !target.is_empty() && target[0] == b':' {
            target = parse::rest(&target, 1);
        }
        if target.is_empty() || dos::seek(self.fd, 0, 0).is_err() {
            return false;
        }
        self.cursor = 0;
        self.count = 0;
        let mut buf = [0; parse::LIMIT];
        while let Some(n) = self.line(&mut buf) {
            let s = parse::left(parse::head(&buf, n));
            if (!s.is_empty() && s[0] == b':')
                && parse::equal(parse::word(parse::rest(&s, 1)).0, target)
            {
                return true;
            }
        }
        false
    }
}
pub fn expand(shell: &Shell, frame: &Frame, line: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut i = 0;
    let mut n = 0;
    while i < line.len() {
        let c = line[i];
        i += 1;
        if c == b'%' && i < line.len() {
            let k = line[i];
            if frame.active() && k == b'%' {
                i += 1;
            } else if frame.active() && k >= b'0' && k <= b'9' {
                if !parse::push(out, &mut n, frame.argument(u16::from(k - b'0'))) {
                    return None;
                }
                i += 1;
                continue;
            } else {
                let mut end = i;
                while end < line.len() && line[end] != b'%' && !parse::space(line[end]) {
                    end += 1;
                }
                if end < line.len() && line[end] == b'%' {
                    let mut value = [0u8; parse::LIMIT];
                    if let Some(add) = shell.env.get(parse::part(line, i, end), &mut value) {
                        if !parse::push(out, &mut n, parse::head(&value, add)) {
                            return None;
                        }
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        if n == out.len() {
            return None;
        }
        out[n] = c;
        n += 1;
    }
    Some(n)
}
pub fn run(shell: &mut Shell, name: &[u8], args: &[u8]) -> u16 {
    if shell.depth >= 8 {
        dos::err(b"Too many nested batch files\r\n");
        return 1;
    }
    let fd = match dos::open(name, 0x80) {
        Ok(fd) => fd,
        Err(_) => return 1,
    };
    let mut frame = Frame::new(fd, name, args);
    shell.depth += 1;
    let mut buf = [0; parse::LIMIT];
    while !frame.done && !shell.exit {
        let Some(n) = frame.line(&mut buf) else { break };
        let mut cmd = parse::left(parse::head(&buf, n));
        if cmd.is_empty() || cmd[0] == b':' {
            continue;
        }
        let quiet = cmd[0] == b'@';
        if quiet {
            cmd = parse::left(parse::rest(&cmd, 1));
        }
        let mut expanded = [0; parse::LIMIT];
        let Some(n) = expand(shell, &frame, cmd, &mut expanded) else {
            execute::syntax(shell);
            continue;
        };
        let line = parse::head(&expanded, n);
        if shell.echo && !quiet {
            prompt::display(&shell.env);
            dos::out(line);
            dos::out(b"\r\n");
        }
        execute::line(shell, &mut frame, line, false);
    }
    shell.depth -= 1;
    dos::close(fd);
    shell.status
}
fn exists(name: &[u8]) -> bool {
    if !parse::contains(name, b'*') && !parse::contains(name, b'?') {
        return dos::exists(name);
    }
    let mut p = [0; 128];
    if !dos::pathbuf(name, &mut p) {
        return false;
    }
    let old = dos::get_dta();
    let mut dta = [0u8; 43];
    dos::set_dta(dos::ds(), dos::ptr(dta.as_mut_ptr()));
    let found = dos::service(0x4e00, 0, 0x37, dos::ptr(p.as_ptr())).is_ok();
    dos::set_dta(old.0, old.1);
    found
}
fn condition(sh: &mut Shell, f: &mut Frame, mut args: &[u8]) {
    let (first, rest) = parse::word(args);
    let invert = parse::equal(first, b"NOT");
    if invert {
        args = rest;
    }
    let (test, rest) = parse::word(args);
    let matches;
    let command;
    if parse::equal(test, b"EXIST") || parse::equal(test, b"ERRORLEVEL") {
        let (operand, tail) = parse::word(rest);
        command = tail;
        if parse::equal(test, b"EXIST") {
            matches = exists(parse::unquote(operand));
        } else {
            let mut n = 0u16;
            if operand.is_empty() {
                execute::syntax(sh);
                return;
            }
            let mut i = 0;
            while i < operand.len() {
                let c = operand[i];
                if c < b'0' || c > b'9' || n > 6552 {
                    execute::syntax(sh);
                    return;
                }
                n = n * 10 + u16::from(c - b'0');
                i += 1;
            }
            matches = sh.status >= n;
        }
    } else {
        let mut i = 0;
        while i + 1 < args.len() && !(args[i] == b'=' && args[i + 1] == b'=') {
            i += 1;
        }
        if i + 1 >= args.len() {
            execute::syntax(sh);
            return;
        }
        let (right, tail) = parse::word(parse::rest(&args, i + 2));
        matches = parse::exact(parse::trim(parse::head(&args, i)), right);
        command = tail;
    }
    if command.is_empty() {
        execute::syntax(sh);
    } else if matches != invert {
        execute::line(sh, f, command, false);
    }
}
fn substitute(sh: &mut Shell, f: &mut Frame, body: &[u8], var: u8, value: &[u8]) {
    let mut buf = [0; parse::LIMIT];
    let mut i = 0;
    let mut n = 0;
    while i < body.len() {
        if body[i] == b'%' && i + 1 < body.len() && body[i + 1] == var {
            if !parse::push(&mut buf, &mut n, value) {
                execute::syntax(sh);
                return;
            }
            i += 2;
        } else {
            if n == buf.len() {
                execute::syntax(sh);
                return;
            }
            buf[n] = body[i];
            n += 1;
            i += 1;
        }
    }
    execute::line(sh, f, parse::head(&buf, n), false);
}
fn for_each(sh: &mut Shell, f: &mut Frame, args: &[u8]) {
    let (var, rest) = parse::word(args);
    let (keyword, rest) = parse::word(rest);
    if var.len() != 2
        || var[0] != b'%'
        || !parse::equal(keyword, b"IN")
        || (rest.is_empty() || rest[0] != b'(')
    {
        execute::syntax(sh);
        return;
    }
    let stop = parse::split_at(parse::rest(&rest, 1), b')') + 1;
    if stop >= rest.len() {
        execute::syntax(sh);
        return;
    }
    let (keyword, body) = parse::word(parse::rest(&rest, stop + 1));
    if !parse::equal(keyword, b"DO") || body.is_empty() {
        execute::syntax(sh);
        return;
    }
    let mut words = parse::left(parse::part(&rest, 1, stop));
    while !words.is_empty() && !f.done && !sh.exit {
        let (item, next) = parse::word(words);
        words = next;
        if !parse::contains(item, b'?') && !parse::contains(item, b'*') {
            substitute(sh, f, body, var[1], item);
            continue;
        }
        let mut path = [0; 128];
        if !dos::pathbuf(item, &mut path) {
            continue;
        }
        let mut prefix = 0;
        let mut j = 0;
        while j < item.len() {
            if item[j] == b':' || item[j] == b'\\' {
                prefix = j + 1;
            }
            j += 1;
        }
        let old = dos::get_dta();
        let mut dta = [0u8; 43];
        let off = dos::ptr(dta.as_mut_ptr());
        dos::set_dta(dos::ds(), off);
        let mut found = dos::service(0x4e00, 0, 0, dos::ptr(path.as_ptr())).is_ok();
        while found && !f.done && !sh.exit {
            let mut name = [0; 128];
            let mut n = 0;
            parse::push(&mut name, &mut n, parse::head(&item, prefix));
            let mut j = 30;
            while j < 43 && dta[j] != 0 && n < name.len() {
                name[n] = dta[j];
                j += 1;
                n += 1;
            }
            substitute(sh, f, body, var[1], parse::head(&name, n));
            dos::set_dta(dos::ds(), off);
            found = dos::service(0x4f00, 0, 0, 0).is_ok();
        }
        dos::set_dta(old.0, old.1);
    }
}
pub fn builtin(sh: &mut Shell, frame: &mut Frame, name: &[u8], args: &[u8]) -> bool {
    if parse::equal(name, b"IF") {
        condition(sh, frame, args);
    } else if parse::equal(name, b"FOR") {
        for_each(sh, frame, args);
    } else if parse::equal(name, b"SHIFT") {
        if frame.active() {
            if frame.shift < 128 {
                frame.shift += 1;
            }
        }
    } else if parse::equal(name, b"GOTO") {
        if frame.active() && !frame.jump(args) {
            dos::err(b"Label not found\r\n");
            frame.done = true;
            sh.status = 1;
        }
    } else if parse::equal(name, b"CALL") {
        if !args.is_empty() {
            execute::line(sh, frame, args, true);
        }
    } else {
        return false;
    }
    true
}
