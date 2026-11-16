//! Native INT 21h file and directory calls through the mounted DOS namespace.
use super::{DosWord, GateToken, LogicalFrame, ResultByte};
use crate::vfs::locks::{self, FileKey, Owner};
use crate::{
    kernel_cell::KernelCell,
    vfs::{dos, fcb},
};
use alloc::{vec, vec::Vec};
type DosResult<T> = Result<T, u16>;

struct Handle {
    file: dos::OpenFile,
    pos: u32,
    readable: bool,
    writable: bool,
}
struct Search {
    dir: u32,
    pattern: [u8; 11],
    attr: u8,
    next: Option<u16>,
}
struct State {
    handles: Vec<Option<Handle>>,
    fcbs: fcb::Table,
    cwd: dos::Directories,
    dta: u64,
    search: Option<Search>,
}
impl State {
    const fn new() -> Self {
        Self {
            handles: Vec::new(),
            fcbs: fcb::Table::new(),
            cwd: dos::Directories::ROOTS,
            dta: 0,
            search: None,
        }
    }
    fn handle(&mut self, id: u16) -> DosResult<&mut Handle> {
        self.handles
            .get_mut(usize::from(id.checked_sub(5).ok_or(6u16)?))
            .and_then(Option::as_mut)
            .ok_or(6)
    }
    fn open(&mut self, dir: u32, name: &[u8; 11], mode: u8, create: Option<u8>) -> DosResult<u16> {
        let index = self
            .handles
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.handles.len());
        if index >= 250 {
            return Err(4);
        }
        let mode = mode & 7;
        if mode > 2 {
            return Err(12);
        }
        if let Some(attr) = create {
            if !dos::write_named_at(dir, name, &[], attr) {
                return Err(5);
            }
        }
        let file = dos::OpenFile::dos_mode(dir, name, mode != 0).map_err(|e| match e {
            dos::FileError::Missing => 2u16,
            dos::FileError::Limit => 4,
            _ => 5,
        })?;
        let h = Some(Handle {
            file,
            pos: 0,
            readable: mode != 1,
            writable: mode != 0,
        });
        if index == self.handles.len() {
            self.handles.push(h)
        } else {
            self.handles[index] = h
        }
        Ok(index as u16 + 5)
    }
    fn next(&mut self, gate: &GateToken) -> DosResult<()> {
        let s = self.search.as_mut().ok_or(18u16)?;
        let f = dos::find_match(s.dir, &s.pattern, s.attr, s.next.ok_or(18u16)?).ok_or(18u16)?;
        s.next = f.index.checked_add(1);
        let mut dta = [0; 43];
        dta[21] = f.attr;
        dta[22..24].copy_from_slice(&f.time.to_le_bytes());
        dta[24..26].copy_from_slice(&f.date.to_le_bytes());
        dta[26..30].copy_from_slice(&f.size.to_le_bytes());
        let name = dos::short_name(&f.name);
        dta[30..30 + name.len()].copy_from_slice(&name);
        gate.copy_out(self.dta, &dta).ok_or(5)
    }
}

static STATE: KernelCell<State> = KernelCell::new(State::new());
pub(super) fn call(gate: &GateToken, frame: &mut LogicalFrame) -> bool {
    let ah = frame.function().0;
    if !matches!(ah,0x0e..=0x17|0x19..=0x1a|0x21..=0x24|0x27..=0x28|0x39..=0x43|0x47|0x4b|0x4e..=0x4f|0x56..=0x57|0x5c)
    {
        return false;
    }
    let mut state = STATE.borrow();
    let previous = dos::directories();
    dos::restore(state.cwd);
    if matches!(ah,0x0f..=0x17|0x21..=0x24|0x27..=0x28) {
        let result = record(gate, &mut state, frame, ah).unwrap_or(0xff);
        frame.set_al(ResultByte(result));
    } else if let Err(error) = handle(gate, &mut state, frame, ah) {
        frame.store_ax(DosWord(error));
    }
    state.cwd = dos::directories();
    dos::restore(previous);
    true
}

fn path(gate: &GateToken, addr: u64) -> DosResult<(u32, [u8; 11])> {
    dos::path_name(&gate.pathname(addr).ok_or(3u16)?).ok_or(3)
}
fn handle(gate: &GateToken, s: &mut State, f: &mut LogicalFrame, ah: u8) -> DosResult<()> {
    let id = f.handle as u16;
    let al = f.status as u8;
    let ax = match ah {
        0x0e => {
            if !dos::select_drive(f.buffer as u8) {
                return Err(15);
            };
            f.set_al(ResultByte(if dos::has_c() { 3 } else { 2 }));
            return Ok(());
        }
        0x19 => {
            f.set_al(ResultByte(dos::active_drive()));
            return Ok(());
        }
        0x1a => {
            s.dta = f.buffer;
            return Ok(());
        }
        0x39 | 0x3a | 0x41 => {
            let (dir, name) = path(gate, f.buffer)?;
            if !dos::in_dir(dir, || match ah {
                0x39 => dos::make_dir(&name),
                0x3a => dos::remove_dir(&name),
                _ => dos::remove_file(&name),
            }) {
                return Err(5);
            };
            0
        }
        0x3b => {
            if !dos::change_dir(&gate.pathname(f.buffer).ok_or(3u16)?) {
                return Err(3);
            };
            0
        }
        0x3c | 0x3d => {
            let (dir, name) = path(gate, f.buffer)?;
            if ah == 0x3c {
                s.open(dir, &name, 2, Some(f.count as u8))?
            } else {
                s.open(dir, &name, al, None)?
            }
        }
        0x3e => {
            if !s.handle(id)?.file.sync() {
                return Err(5);
            }
            locks::discard(|owner| owner == Owner::Gate(id));
            s.handles[usize::from(id - 5)] = None;
            0
        }
        0x3f | 0x40 => {
            let h = s.handle(id)?;
            if locks::blocked(
                FileKey(h.file.identity()),
                Owner::Gate(id),
                h.pos,
                u32::from(f.count as u16),
            ) {
                return Err(0x21);
            }
            let mut bytes = vec![0; usize::from(f.count as u16)];
            let n = if ah == 0x3f {
                if !h.readable {
                    return Err(5);
                }
                let n = h.file.read(h.pos as usize, &mut bytes).map_err(|_| 5u16)?;
                gate.copy_out(f.buffer, &bytes[..n]).ok_or(5u16)?;
                n
            } else {
                if !h.writable {
                    return Err(5);
                }
                gate.copy_in(f.buffer, &mut bytes).ok_or(5u16)?;
                h.file.write(h.pos as usize, &bytes).map_err(|_| 5u16)?;
                bytes.len()
            };
            h.pos = h.pos.saturating_add(n as u32);
            n as u16
        }
        0x42 => {
            let h = s.handle(id)?;
            let delta = (u32::from(f.count as u16) << 16) | u32::from(f.buffer as u16);
            h.pos = match al {
                0 => Some(delta),
                1 => h.pos.checked_add_signed(delta as i32),
                2 => (h.file.len() as u32).checked_add_signed(delta as i32),
                _ => return Err(1),
            }
            .ok_or(1u16)?;
            f.store_dx(DosWord((h.pos >> 16) as u16));
            h.pos as u16
        }
        0x43 => {
            let (dir, name) = path(gate, f.buffer)?;
            match al {
                0 => f.store_cx(DosWord(u16::from(
                    dos::in_dir(dir, || dos::attribute(&name)).ok_or(2u16)?,
                ))),
                1 => {
                    if !dos::in_dir(dir, || dos::set_attribute(&name, f.count as u8)) {
                        return Err(5);
                    }
                }
                _ => return Err(1),
            };
            0
        }
        0x47 => {
            let drive = (f.buffer as u8)
                .checked_sub(1)
                .unwrap_or_else(dos::active_drive);
            let dir = dos::drive_directory(drive).ok_or(15u16)?;
            let mut buf = [0; 64];
            let n = dos::in_dir(dir, || dos::current_path(&mut buf[..63]));
            gate.copy_out(f.source, &buf[..=n]).ok_or(5u16)?;
            0
        }
        0x4b => {
            if !crate::dos::exec::function_ok(al) {
                return Err(1);
            }
            let (dir, name) = path(gate, f.buffer)?;
            // Preserve this native gate's COMMAND.COM probe. Guest EXEC performs program loading.
            if name != *b"COMMAND COM" {
                return Err(2);
            }
            dos::OpenFile::open(dir, &name, false).map_err(|_| 2u16)?;
            0
        }
        0x4e | 0x4f => {
            if ah == 0x4e {
                let (dir, pattern) =
                    dos::search_path(&gate.pathname(f.buffer).ok_or(3u16)?).ok_or(3u16)?;
                s.search = Some(Search {
                    dir,
                    pattern,
                    attr: f.count as u8,
                    next: Some(0),
                });
            }
            s.next(gate)?;
            0
        }
        0x56 => {
            let (dir, from) = path(gate, f.buffer)?;
            let (dest, to) = path(gate, f.destination)?;
            if dir != dest || !dos::in_dir(dir, || dos::rename_file(&from, &to)) {
                return Err(5);
            };
            0
        }
        0x57 => {
            let h = s.handle(id)?;
            match al {
                0 => {
                    let (t, d) = h.file.stamp();
                    f.store_cx(DosWord(t));
                    f.store_dx(DosWord(d))
                }
                1 => {
                    if !h.writable || !h.file.set_stamp(f.count as u16, f.buffer as u16) {
                        return Err(5);
                    }
                }
                _ => return Err(1),
            };
            0
        }
        0x5c => {
            let key = FileKey(s.handle(id)?.file.identity());
            let start = (u32::from(f.count as u16) << 16) | u32::from(f.buffer as u16);
            let len = (u32::from(f.source as u16) << 16) | u32::from(f.destination as u16);
            locks::change(key, Owner::Gate(id), al, start, len).unwrap_or(0)
        }
        _ => return Err(1),
    };
    f.store_ax(DosWord(ax));
    Ok(())
}
fn record(gate: &GateToken, s: &mut State, f: &mut LogicalFrame, ah: u8) -> Option<u8> {
    let mut first = [0];
    gate.copy_in(f.buffer, &mut first)?;
    let mut addr = f.buffer;
    let attr = if first[0] == 0xff {
        gate.copy_in(addr.checked_add(6)?, &mut first)?;
        addr = addr.checked_add(7)?;
        first[0]
    } else {
        0
    };
    let mut raw = [0; 37];
    gate.copy_in(addr, &mut raw[..33])?;
    let width = if u16::from_le_bytes([raw[14], raw[15]]) >= 64 {
        36
    } else {
        37
    };
    if matches!(ah,0x21..=0x24|0x27..=0x28) {
        gate.copy_in(addr.checked_add(33)?, &mut raw[33..width])?;
    }
    let old = raw;
    let (status, done) = fcb::service(
        &mut s.fcbs,
        0,
        &mut raw,
        ah,
        attr,
        f.count as u16,
        65536,
        |index, buf, input| {
            let Some(at) = s.dta.checked_add(index as u64).filter(|_| s.dta != 0) else {
                return false;
            };
            if input {
                gate.copy_in(at, buf).is_some()
            } else {
                gate.copy_out(at, buf).is_some()
            }
        },
    );
    for (i, (&old, &b)) in old.iter().zip(&raw).enumerate() {
        if b != old {
            gate.copy_out(addr.checked_add(i as u64)?, &[b])?;
        }
    }
    if matches!(ah, 0x27 | 0x28) {
        f.store_cx(DosWord(done))
    }
    Some(status)
}
