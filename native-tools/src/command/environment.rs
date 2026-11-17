//! Each COMMAND process owns its DOS environment and passes it to ordinary EXEC.
use super::{dos, parse};
use dosrs_realmode::{byte, store, system::sw};
pub struct Environment {
    pub segment: u16,
    end: u16,
    path: [u8; 128],
    path_len: u16,
    capacity: u16,
}
impl Environment {
    pub fn new(psp: u16, capacity: u16) -> Option<Self> {
        let source = dos::inherited_env(psp);
        // Keep the resident environment out of the largest conventional block
        // when the host has linked UMBs. DOS otherwise allocates low memory.
        let strategy = dos::service(0x5800, 0, 0, 0).ok();
        let upper_first =
            strategy.is_some_and(|s| dos::service(0x5801, 0x80 | (s & 3), 0, 0).is_ok());
        let allocation = dos::service(0x4800, capacity >> 4, 0, 0);
        if upper_first {
            dos::service(0x5801, strategy.unwrap_or(0), 0, 0).ok()?;
        }
        let seg = allocation.ok()?;
        let mut env = Self {
            segment: seg,
            end: 0,
            path: [0; 128],
            path_len: 0,
            capacity,
        };
        if source != 0 {
            let mut pos = 0u16;
            while pos < 32767 && byte(source, pos) != 0 {
                let start = pos;
                while pos < 32767 && byte(source, pos) != 0 {
                    pos += 1;
                }
                if pos >= 32767 {
                    return None;
                }
                let n = pos - start + 1;
                if env.end + n + 134 > capacity {
                    return None;
                }
                let mut i = 0;
                while i < n {
                    store(seg, env.end + i, byte(source, start + i));
                    i += 1;
                }
                env.end += n;
                pos += 1;
            }
            let at = if pos == 0 { 2 } else { pos + 1 };
            if byte(source, at) == 1 && byte(source, at + 1) == 0 {
                while env.path_len < 127 {
                    let i = env.path_len;
                    let c = byte(source, at + 2 + i);
                    if c == 0 {
                        break;
                    }
                    env.path[i as usize] = c as u8;
                    env.path_len += 1;
                }
            }
        }
        env.finish();
        sw(psp, 0x2c, seg);
        if env.find(b"COMSPEC").is_none() {
            let mut path = [0u8; 128];
            let n = if env.path_len > 0 {
                let mut i = 0;
                while i < env.path_len {
                    path[i as usize] = env.path[i as usize];
                    i += 1;
                }
                env.path_len as usize
            } else {
                path[0] = b'A' + dos::drive() as u8;
                let mut n = 1;
                parse::push(&mut path, &mut n, b":\\COMMAND.COM");
                n
            };
            env.assign(b"COMSPEC", parse::head(&path, n));
        }
        Some(env)
    }
    fn finish(&self) {
        let mut at = self.end;
        if at == 0 {
            store(self.segment, at, 0);
            at += 1;
        }
        store(self.segment, at, 0);
        sw(self.segment, at + 1, 1);
        at += 3;
        let mut i = 0;
        while i < self.path_len {
            store(self.segment, at + i, u16::from(self.path[i as usize]));
            i += 1;
        }
        store(self.segment, at + i, 0);
    }
    fn find(&self, key: &[u8]) -> Option<(u16, u16)> {
        let mut from = 0;
        while from < self.end {
            let mut p = from;
            while p < self.end && byte(self.segment, p) != 0 {
                p += 1;
            }
            if p - from > key.len() as u16 && byte(self.segment, from + key.len() as u16) == 61 {
                let mut i = 0;
                while i < key.len()
                    && parse::upper(byte(self.segment, from + i as u16) as u8)
                        == parse::upper(key[i])
                {
                    i += 1;
                }
                if i == key.len() {
                    return Some((from, p));
                }
            }
            from = p + 1;
        }
        None
    }
    pub fn get(&self, key: &[u8], dest: &mut [u8]) -> Option<usize> {
        let (start, end) = self.find(key)?;
        let val = start + key.len() as u16 + 1;
        let n = (end - val) as usize;
        if n > dest.len() {
            return None;
        }
        let mut i = 0;
        while i < n {
            dest[i] = byte(self.segment, val + i as u16) as u8;
            i += 1;
        }
        Some(n)
    }
    pub fn assign(&mut self, key: &[u8], value: &[u8]) -> bool {
        if key.is_empty() {
            return false;
        }
        let old = self.find(key);
        let removed = if let Some((from, to)) = old {
            to - from + 1
        } else {
            0
        };
        let added = if value.is_empty() {
            0
        } else {
            key.len() + value.len() + 2
        };
        if self.end - removed + added as u16 + 5 + self.path_len > self.capacity {
            return false;
        }
        if let Some((from, to)) = old {
            let mut to = to + 1;
            let mut at = from;
            while to < self.end {
                store(self.segment, at, byte(self.segment, to));
                at += 1;
                to += 1;
            }
            self.end -= removed;
        }
        if !value.is_empty() {
            let mut i = 0;
            while i < key.len() {
                store(self.segment, self.end, parse::upper(key[i]) as u16);
                self.end += 1;
                i += 1;
            }
            store(self.segment, self.end, 61);
            self.end += 1;
            i = 0;
            while i < value.len() {
                store(self.segment, self.end, value[i] as u16);
                self.end += 1;
                i += 1;
            }
            store(self.segment, self.end, 0);
            self.end += 1;
        }
        self.finish();
        true
    }
    pub fn list(&self) {
        let mut i = 0;
        let mut buf = [0u8; 64];
        let mut n = 0;
        while i < self.end {
            let c = byte(self.segment, i);
            i += 1;
            if c == 0 {
                dos::out(parse::head(&buf, n));
                dos::out(b"\r\n");
                n = 0;
            } else {
                buf[n] = c as u8;
                n += 1;
                if n == buf.len() {
                    dos::out(&buf);
                    n = 0;
                }
            }
        }
    }
}
