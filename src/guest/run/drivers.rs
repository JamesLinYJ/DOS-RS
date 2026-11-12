//! DOS device headers, resident memory and strategy/interrupt request packets.
use super::*;
use crate::bios::pc;
use crate::guest::disk;

const DOS_OWNER: u16 = 8;
const STACK_END: u16 = 0x800;
const REQUEST_BYTES: usize = 30;
const DEVICE_LIMIT: usize = 16;

#[derive(Clone, Copy)]
pub(super) struct Header {
    segment: u16,
    offset: u16,
    strategy: u16,
    interrupt: u16,
    attributes: u16,
    name: [u8; 8],
    first_drive: u8,
    units: u8,
}

pub(super) struct Drivers {
    headers: [Option<Header>; DEVICE_LIMIT],
    scratch: u16,
    drive_count: u8,
}

#[derive(Clone, Copy)]
pub(super) enum Device {
    Console,
    Null,
    Serial(u16),
    Printer,
    Resident(Header),
}

impl Drivers {
    pub(super) fn new() -> Self {
        Self {
            headers: [None; DEVICE_LIMIT],
            scratch: 0,
            drive_count: 2,
        }
    }

    pub(super) fn find(&self, name: &[u8; 11]) -> Option<Device> {
        let base = &name[..8];
        for header in self.headers.iter().rev().flatten() {
            if header.attributes & 0x8000 != 0 && header.name.eq_ignore_ascii_case(base) {
                return Some(Device::Resident(*header));
            }
        }
        Some(match base {
            b"CON     " => Device::Console,
            b"NUL     " => Device::Null,
            b"COM1    " | b"AUX     " => Device::Serial(pc::UARTS[0]),
            b"COM2    " => Device::Serial(pc::UARTS[1]),
            b"LPT1    " | b"PRN     " => Device::Printer,
            _ => return None,
        })
    }

    // FreeDOS SETVER publishes a length-prefixed name/version table at LoL+37h.
    pub(super) fn program_version(&self, ram: &GuestRam, name: &[u8; 11]) -> Option<u16> {
        let (kernel, list) = self.list_address();
        let mut at = read_word(ram, kernel, list + 0x37)?;
        let segment = read_word(ram, kernel, list + 0x39)?;
        if segment == 0 && at == 0 {
            return None;
        }
        let mut filename = [0; 12];
        let base = name[..8].iter().position(|&b| b == b' ').unwrap_or(8);
        let ext = name[8..].iter().position(|&b| b == b' ').unwrap_or(3);
        filename[..base].copy_from_slice(&name[..base]);
        let len = if ext == 0 {
            base
        } else {
            filename[base] = b'.';
            filename[base + 1..base + 1 + ext].copy_from_slice(&name[8..8 + ext]);
            base + 1 + ext
        };
        loop {
            let n = u16::from(ram.load(segment, at)?);
            if n == 0 {
                return None;
            }
            let start = at.checked_add(1)?;
            let version = start.checked_add(n)?;
            if usize::from(n) == len
                && filename[..len].iter().enumerate().all(|(i, b)| {
                    ram.load(segment, start + i as u16)
                        .is_some_and(|v| b.eq_ignore_ascii_case(&v))
                })
            {
                return read_word(ram, segment, version).filter(|v| *v != 0);
            }
            at = version.checked_add(2)?;
        }
    }

    pub(super) fn is_raw_volume(&self, letter: u8) -> bool {
        let drive = letter.to_ascii_uppercase().wrapping_sub(b'A');
        (2..self.drive_count).contains(&drive)
    }

    pub(super) fn list_address(&self) -> (u16, u16) {
        // Installable drivers such as FreeDOS SETVER expect the request and
        // list of lists to share the kernel's data segment across INT 21h.
        if self.scratch == 0 {
            (0x50, 0x10)
        } else {
            (self.scratch, 0x200)
        }
    }

    pub(super) fn head(&self) -> [u8; 4] {
        self.headers
            .iter()
            .rev()
            .flatten()
            .next()
            .map_or([0xff; 4], |h| {
                let [lo, hi] = h.segment.to_le_bytes();
                let [off_lo, off_hi] = h.offset.to_le_bytes();
                [off_lo, off_hi, lo, hi]
            })
    }
}

pub(super) fn configure(ram: &mut GuestRam, session: &mut Session) {
    let mut config = [0; 4096];
    let name = b"CONFIG  SYS";
    if disk::file_len(name).is_none() {
        return;
    }
    let Some(size) = disk::read_file(name, &mut config) else {
        crate::puts("Cannot read CONFIG.SYS\n");
        return;
    };
    for line in config[..size].split(|b| matches!(b, b'\r' | b'\n')) {
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        let (key, rest) = (&line[..eq], &line[eq + 1..]);
        let key = key.trim_ascii();
        if !key.eq_ignore_ascii_case(b"DEVICE") && !key.eq_ignore_ascii_case(b"DEVICEHIGH") {
            continue;
        }
        let spec = rest.trim_ascii();
        let filename = spec
            .split(u8::is_ascii_whitespace)
            .next()
            .unwrap_or_default();
        let result = install(filename, spec, ram, session);
        crate::puts(if result.is_ok() {
            "Loaded driver "
        } else {
            "Cannot load driver "
        });
        crate::bios::mscon::con_write_bytes(filename);
        if let Err(code) = result {
            crate::puts(" (DOS error ");
            crate::bios::mscon::write_number(code as u8);
            crate::puts(")");
        }
        crate::puts("\n");
    }
}

fn install(
    filename: &[u8],
    command: &[u8],
    ram: &mut GuestRam,
    session: &mut Session,
) -> Result<(), u16> {
    if filename.is_empty() || filename.len() >= 64 || command.len() > 250 {
        return Err(11);
    }
    let mut path = [0; 64];
    path[..filename.len()].copy_from_slice(filename);
    let (dir, name) = search_spec(&path).ok_or(3u16)?;
    let slot = session
        .drivers
        .headers
        .iter()
        .position(Option::is_none)
        .ok_or(8u16)?;
    if session.drivers.scratch == 0 {
        session.drivers.scratch = session
            .arena
            .alloc(DOS_OWNER, STACK_END / 16, 0)
            .map_err(|_| 8u16)?;
    }
    // Allow INIT to use the rest of a 64 KiB segment for uninitialized data.
    let segment = session
        .arena
        .alloc(DOS_OWNER, 0x1000, 0)
        .map_err(|_| 8u16)?;
    let scratch = session.drivers.scratch;
    let mut saved_list = [0u8; 64];
    saved_list.copy_from_slice(ram.paragraphs_mut(scratch + 0x20, scratch + 0x24));
    let result = (|| {
        let size = {
            let bytes = ram.paragraphs_mut(segment, segment + 0x1000);
            bytes.fill(0);
            let mut image = super::EXEC_IMAGE.borrow();
            let length = disk::read_file_at(dir, &name, &mut image[..]).ok_or(2u16)?;
            let file = &image[..length];
            if file.starts_with(b"MZ") {
                crate::guest::mz::Module::parse(file)
                    .and_then(|m| m.copy_to(bytes, segment))
                    .ok_or(11u16)?
            } else {
                bytes.get_mut(..length).ok_or(11u16)?.copy_from_slice(file);
                length
            }
        };
        let mut pending: [Option<Header>; DEVICE_LIMIT] = [None; DEVICE_LIMIT];
        let mut offset = 0u16;
        let mut drive = session.drivers.drive_count;
        let mut top = u32::from(segment + 0x1000) * 16;
        let mut needed = 0;
        for index in 0..DEVICE_LIMIT - slot {
            if pending[..index]
                .iter()
                .flatten()
                .any(|h| h.offset == offset)
            {
                return Err(11);
            }
            let bytes = ram
                .paragraphs_mut(segment, segment + 0x1000)
                .get(usize::from(offset)..size)
                .and_then(|b| b.get(..18))
                .ok_or(11u16)?;
            let next = u16::from_le_bytes([bytes[0], bytes[1]]);
            let mut header = Header {
                segment,
                offset,
                attributes: u16::from_le_bytes([bytes[4], bytes[5]]),
                strategy: u16::from_le_bytes([bytes[6], bytes[7]]),
                interrupt: u16::from_le_bytes([bytes[8], bytes[9]]),
                name: bytes[10..18].try_into().unwrap(),
                first_drive: drive,
                units: 0,
            };
            if next == offset || usize::from(header.strategy.max(header.interrupt)) >= size {
                return Err(11);
            }
            needed = needed
                .max(u32::from(offset) + 18)
                .max(u32::from(header.strategy.max(header.interrupt)) + 1);
            let scratch = session.drivers.scratch;
            let work = ram.paragraphs_mut(scratch, scratch + STACK_END / 16);
            work[..REQUEST_BYTES].fill(0);
            work[0] = REQUEST_BYTES as u8;
            work[14..16].copy_from_slice(&((top % 16) as u16).to_le_bytes());
            work[16..18].copy_from_slice(&((top / 16) as u16).to_le_bytes());
            work[18..20].copy_from_slice(&64u16.to_le_bytes());
            work[20..22].copy_from_slice(&scratch.to_le_bytes());
            work[22] = header.first_drive;
            work[64..64 + command.len()].copy_from_slice(command);
            work[64 + command.len()..66 + command.len()].copy_from_slice(b"\r\n");
            work[66 + command.len()] = 0;
            execute(header, ram, session)?;
            let end = u32::from(read_word(ram, scratch, 16).unwrap()) * 16
                + u32::from(read_word(ram, scratch, 14).unwrap());
            top = end;
            if header.attributes & 0x8000 == 0 {
                header.units = ram.load(scratch, 13).unwrap();
                if header.units == 0 || header.units > 26 - drive {
                    return Err(15);
                }
                drive += header.units;
            }
            pending[index] = Some(header);
            offset = next;
            if next == 0xffff {
                break;
            }
        }
        // No headers become visible until the whole image initializes successfully.
        let resident = top.checked_sub(u32::from(segment) * 16).ok_or(11u16)?;
        if offset != 0xffff || !(needed..=65536).contains(&resident) {
            return Err(11);
        }
        session
            .arena
            .resize(DOS_OWNER, segment, resident.div_ceil(16) as u16)
            .map_err(|_| 8u16)?;
        for (index, header) in pending.into_iter().flatten().enumerate() {
            let at = usize::from(header.offset);
            ram.paragraphs_mut(segment, segment + resident.div_ceil(16) as u16)[at..at + 4]
                .copy_from_slice(&session.drivers.head());
            session.drivers.headers[slot + index] = Some(header);
        }
        session.drivers.drive_count = drive;
        Ok(())
    })();
    if result.is_err() {
        ram.paragraphs_mut(scratch + 0x20, scratch + 0x24)
            .copy_from_slice(&saved_list);
        let _ = session.arena.free(DOS_OWNER, segment);
    }
    result
}

fn execute(header: Header, ram: &mut GuestRam, session: &mut Session) -> Result<u16, u16> {
    let scratch = session.drivers.scratch;
    let mut cpu = CpuState::at_com(header.segment);
    cpu.set_selector(Segment::Extra, scratch);
    cpu.set_selector(Segment::Stack, scratch);
    cpu.set_wide(Slot::Stack, STACK_END);
    for address in [header.strategy, header.interrupt] {
        cpu.set_selector(Segment::Data, header.segment);
        cpu.set_wide(Slot::Source, header.offset);
        cpu.set_selector(Segment::Code, header.segment);
        cpu.set_instruction(address);
        // A far return to this reserved offset marks completion of each call.
        guest_push(&mut cpu, ram, scratch);
        guest_push(&mut cpu, ram, 0x40);
        let mut returned = false;
        for _ in 0..100_000 {
            if cpu.selector(Segment::Code) == scratch && cpu.instruction() == 0x40 {
                returned = true;
                break;
            }
            match step::step(&mut cpu, ram) {
                Outcome::Continue => {}
                Outcome::Interrupt(0x21)
                    if matches!(cpu.high(Slot::Accumulator), 0x02 | 0x09 | 0x30 | 0x52) =>
                {
                    service(&mut cpu, ram, session);
                }
                Outcome::Interrupt(v) if serve_bios(&mut cpu, ram, v) => {}
                _ => return Err(11),
            }
        }
        if !returned {
            return Err(11);
        }
    }
    let status = read_word(ram, scratch, 3).ok_or(11u16)?;
    if status & 0x8100 == 0x0100 {
        Ok(status)
    } else if status & 0x8000 != 0 {
        Err(match status as u8 {
            0 => 19,
            1 => 20,
            2 => 21,
            3 => 22,
            4..=12 => 19 + u16::from(status as u8),
            _ => 31,
        })
    } else {
        Err(31)
    }
}

struct Request {
    command: u8,
    unit: u8,
    buffer: (u16, u16),
    count: u16,
    sector: u16,
}

fn transfer(
    header: Header,
    req: Request,
    ram: &mut GuestRam,
    session: &mut Session,
) -> Result<u16, u16> {
    let scratch = session.drivers.scratch;
    let packet = ram.paragraphs_mut(scratch, scratch + 2);
    packet[..REQUEST_BYTES].fill(0);
    packet[0] = REQUEST_BYTES as u8;
    packet[1] = req.unit;
    packet[2] = req.command;
    packet[14..16].copy_from_slice(&req.buffer.1.to_le_bytes());
    packet[16..18].copy_from_slice(&req.buffer.0.to_le_bytes());
    packet[18..20].copy_from_slice(&req.count.to_le_bytes());
    packet[20..22].copy_from_slice(&req.sector.to_le_bytes());
    execute(header, ram, session)?;
    read_word(ram, scratch, 18)
        .filter(|n| *n <= req.count)
        .ok_or(31)
}

pub(super) fn ioctl(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) -> bool {
    let Some(device) = copy_handle(session, cpu.wide(Slot::Base)).and_then(|f| f.device) else {
        return false;
    };
    let function = cpu.low(Slot::Accumulator);
    let header = if let Device::Resident(h) = device {
        Some(h)
    } else {
        None
    };
    let result = match function {
        0 => {
            let attrs = match device {
                Device::Resident(h) => h.attributes & 0x400f,
                Device::Console => 3,
                Device::Null => 4,
                _ => 0,
            };
            cpu.set_wide(Slot::Data, 0x80 | attrs);
            Ok(())
        }
        2 | 3 => {
            let result = header
                .filter(|h| h.attributes & 0x4000 != 0)
                .ok_or(1u16)
                .and_then(|h| {
                    let req = Request {
                        command: if function == 2 { 3 } else { 12 },
                        unit: 0,
                        buffer: (cpu.selector(Segment::Data), cpu.wide(Slot::Data)),
                        count: cpu.wide(Slot::Count),
                        sector: 0,
                    };
                    transfer(h, req, ram, session)
                });
            result.map(|n| cpu.set_wide(Slot::Accumulator, n))
        }
        6 | 7 => {
            let ready = match device {
                Device::Resident(h) => {
                    let packet =
                        ram.paragraphs_mut(session.drivers.scratch, session.drivers.scratch + 2);
                    packet[..REQUEST_BYTES].fill(0);
                    packet[0] = 13;
                    packet[2] = if function == 6 { 6 } else { 10 };
                    execute(h, ram, session).map(|s| s & 0x200 == 0)
                }
                Device::Console => Ok(function == 7 || pc::key(false).is_some()),
                Device::Serial(base) => {
                    let status = (pc::serial_status(base) >> 8) as u8;
                    Ok(status != 0xff && status & (if function == 6 { 1 } else { 0x20 }) != 0)
                }
                Device::Printer => Ok(function == 7 && pc::printer_status() & 0x80 != 0),
                Device::Null => Ok(true),
            };
            ready.map(|yes| cpu.set_low(Slot::Accumulator, if yes { 0xff } else { 0 }))
        }
        _ => return false,
    };
    match result {
        Ok(()) => cpu.set_carry(false),
        Err(e) => fail(cpu, session, e),
    }
    true
}

pub(super) fn absolute(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session, write: bool) {
    let drive = cpu.low(Slot::Accumulator);
    let count = cpu.wide(Slot::Count);
    // INT 25h/26h leave the caller's original FLAGS on the stack.
    guest_push(cpu, ram, cpu.flags());
    let header = session
        .drivers
        .headers
        .iter()
        .flatten()
        .copied()
        .find(|h| drive >= h.first_drive && drive - h.first_drive < h.units);
    let result = header.ok_or(15u16).and_then(|h| {
        if count == 0xffff {
            return Err(1);
        }
        let req = Request {
            command: if write { 8 } else { 4 },
            unit: drive - h.first_drive,
            buffer: (cpu.selector(Segment::Data), cpu.wide(Slot::Base)),
            count,
            sector: cpu.wide(Slot::Data),
        };
        transfer(h, req, ram, session).and_then(|done| if done == count { Ok(()) } else { Err(31) })
    });
    if let Err(code) = result {
        cpu.set_wide(Slot::Accumulator, code);
    }
    cpu.set_carry(result.is_err());
}

pub(super) fn handle_io(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    session: &mut Session,
    write: bool,
) -> bool {
    let handle = cpu.wide(Slot::Base);
    let Some(opened) = copy_handle(session, handle) else {
        return false;
    };
    let Some(device) = opened.device else {
        return false;
    };
    if (write && opened.access == 0) || (!write && opened.access == 1) {
        fail(cpu, session, 5);
        return true;
    }
    let seg = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    let count = cpu.wide(Slot::Count);
    let result = match device {
        Device::Resident(header) => transfer(
            header,
            Request {
                command: if write { 8 } else { 4 },
                unit: 0,
                buffer: (seg, offset),
                count,
                sector: 0,
            },
            ram,
            session,
        ),
        _ => {
            let mut done = 0;
            while done < count {
                let at = offset.wrapping_add(done);
                if write {
                    let byte = ram.load(seg, at).unwrap();
                    match device {
                        Device::Console => crate::serial_putc(byte),
                        Device::Serial(base) if !pc::serial_write(base, byte) => break,
                        Device::Printer if !pc::print_byte(byte) => break,
                        _ => {}
                    }
                } else {
                    let byte = match device {
                        Device::Console => pc::key(true).map(|word| word as u8),
                        Device::Serial(base) => pc::serial_read(base),
                        _ => None,
                    };
                    let Some(byte) = byte else { break };
                    ram.store(seg, at, byte);
                }
                done += 1;
            }
            Ok(done)
        }
    };
    match result {
        Ok(done) => {
            cpu.set_wide(Slot::Accumulator, done);
            cpu.set_carry(false);
        }
        Err(code) => fail(cpu, session, code),
    }
    true
}
