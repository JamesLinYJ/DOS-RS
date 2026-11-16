//! Produce only the startup directives understood by this native kernel.
use dosrs_realmode::{store, system::Media};
pub const CONFIG: &[u8] = b"CONFIG  SYS";
pub const AUTOEXEC: &[u8] = b"AUTOEXECBAT";
#[derive(Clone, Copy)]
pub struct Settings {
    pub display: u16,
    pub language: u16,
    pub cache: bool,
    pub cd: bool,
    pub batch: u16,
}
impl Settings {
    pub fn defaults() -> Self {
        Self {
            display: 0,
            language: dosrs_realmode::language::ENGLISH,
            cache: true,
            cd: true,
            batch: 0,
        }
    }
    pub fn mode(&self) -> &'static str {
        match self.display {
            1 => "CGA",
            2 => "EGA",
            3 => "VGA",
            4 => "MONO",
            _ => "AUTO",
        }
    }
    pub fn render(&self, media: &Media, index: u16, seg: u16) -> u16 {
        let mut n = 0;
        if index == 0 {
            write(seg, &mut n, "LANG=");
            write(seg, &mut n, dosrs_realmode::language::tag(self.language));
            write(seg, &mut n, "\r\nDISPLAY=");
            write(seg, &mut n, self.mode());
            write(seg, &mut n, "\r\nSMARTDRV=");
            write(seg, &mut n, if self.cache { "ON" } else { "OFF" });
            write(seg, &mut n, "\r\nCDROM=");
            write(seg, &mut n, if self.cd { "ON" } else { "OFF" });
            write(seg, &mut n, "\r\nDEVICE=EMM386.SYS\r\n");
        } else if self.batch == 2 {
            let Some(f) = media.lookup(b"AUTOEXECBAT") else {
                return 0;
            };
            if f.size > 2048 || f.size == 0 || !media.fetch(&f, 0, f.size as u16, seg, 0) {
                return 0;
            }
            n = f.size as u16;
        } else if self.batch == 1 {
            write(seg, &mut n, "REM Minimal startup\r\n")
        } else {
            write(seg, &mut n, "REM DOS startup\r\nVER\r\n");
            if self.cache {
                write(seg, &mut n, "SMARTDRV /STATUS\r\n")
            }
        }
        let mut pad = n;
        while pad & 511 != 0 {
            store(seg, pad, 0);
            pad += 1;
        }
        n
    }
}
fn write(seg: u16, n: &mut u16, s: &str) {
    let mut i = 0;
    while i < s.len() {
        store(seg, *n, s.as_bytes()[i] as u16);
        *n += 1;
        i += 1;
    }
}
