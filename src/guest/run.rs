//! Synchronous guest execution, DOS services and modeled BIOS interrupts.
//! Boot fixtures live in the smoke submodule.

use super::arena::{Arena, MemoryError};
use super::memory::{GuestRam, read_word};
use super::process::{CommandTail, Program};
use super::state::{CpuState, Segment, Slot};
use super::step::{self, Outcome};

use crate::kernel_cell::KernelCell;

mod drivers;
pub(super) mod ems;
mod fcb;
mod handles;
mod locks;
mod runtime;
mod termination;
pub(super) mod xms;
use fcb::{
    fcb_block_read, fcb_block_write, fcb_close, fcb_create, fcb_delete, fcb_find, fcb_open,
    fcb_place, fcb_random_read, fcb_random_write, fcb_read, fcb_rename, fcb_size, fcb_write,
};
mod bios;
use bios::serve_bios;
mod smoke;
pub(crate) use runtime::worker_tick;
pub use runtime::{interactive, run_shell};
pub use smoke::show;

const LAUNCH_SEGMENT: u16 = 0x1600;
const SHELL_SEGMENT: u16 = 0x0900;
const SHELL_CHILD: u16 = 0x0B00;

fn load_root(
    image: &[u8],
    name: &[u8; 11],
    preferred_psp: u16,
    args: &[u8],
    ram: &mut GuestRam,
    session: &mut Session,
) -> Option<CpuState> {
    let program = Program {
        directory: b"A:\\",
        name,
        image,
        preferred_psp,
        com_paragraphs: if preferred_psp == SHELL_SEGMENT {
            0x100
        } else {
            0x1000
        },
        environment: 0,
        parent: 0,
        tail: CommandTail::arguments(args),
    };
    let (psp, cpu) = program.load(ram, &mut session.arena).ok()?;
    set_psp_version(ram, psp, crate::dos::getset::VERSION.0);
    session.handles.publish(ram, psp);
    session.prefix = psp;
    session.current_psp = psp;
    session.dta_segment = psp;
    session.dta_offset = 0x80;
    Some(cpu)
}

enum End {
    Waiting,
    Switch,
    Stopped(u8),
    Fault(u8),
    Other(u8),
    Loop,
}

fn drive(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session, limit: u32) -> End {
    let mut steps = 0u32;
    while steps < limit {
        steps += 1;
        let (event, instruction) = step::slice(cpu, ram, 1);
        if let Some(end) = handle_event(event, instruction, cpu, ram, session) {
            return end;
        }
        if session.pending.is_some() || session.batches.ready(session.child_depth) {
            return End::Switch;
        }
    }
    End::Loop
}

fn serve_port(cpu: &mut CpuState, ram: &mut GuestRam, port: u16, write: bool, word: bool) {
    use crate::bios::pc::{port_read, port_write};
    for i in 0..=u16::from(word) {
        let port = port.wrapping_add(i);
        let value = if i == 0 {
            cpu.low(Slot::Accumulator)
        } else {
            cpu.high(Slot::Accumulator)
        };
        let result = ram.system_port(port, write, value).unwrap_or_else(|| {
            if write {
                port_write(port, value);
                0
            } else {
                port_read(port)
            }
        });
        if !write {
            if i == 0 {
                cpu.set_low(Slot::Accumulator, result);
            } else {
                cpu.set_high(Slot::Accumulator, result);
            }
        }
    }
}

fn handle_event(
    event: Outcome,
    instruction: u16,
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    session: &mut Session,
) -> Option<End> {
    match event {
        #[cfg(feature = "pc386")]
        Outcome::Firmware(v) => {
            if (v == 0x16 && matches!(cpu.high(Slot::Accumulator), 0 | 0x10)
                || v == 0x21 && matches!(cpu.high(Slot::Accumulator), 1 | 7 | 8))
                && crate::bios::pc::key(false).is_none()
            {
                return session.scheduled.then_some(End::Waiting);
            }
            if step::interrupt_return(cpu, ram, 0xcf) != Outcome::Continue {
                return Some(End::Fault(0xcf));
            }
            if matches!(v, 0x08 | 0x09) {
                if let Some(at) = &mut ram.at {
                    at.eoi();
                }
                if v == 8 {
                    let low = read_word(ram, 0x40, 0x6c).unwrap();
                    let high = read_word(ram, 0x40, 0x6e).unwrap();
                    let count = ((u32::from(high) << 16) | u32::from(low)) + 1;
                    store_bytes(ram, 0x40, 0x6c, &(count % 0x1800b0).to_le_bytes());
                    if count == 0x1800b0 {
                        ram.store(0x40, 0x70, 1);
                    }
                }
                return None;
            }
            if matches!(v, 0x1c | 0x28 | 0x2a) {
                return None;
            }
            // A DOS extender can call the old mouse interrupt vector directly.
            if v == 0x33 {
                if matches!(cpu.wide(Slot::Accumulator), 0 | 0x21) {
                    cpu.set_wide(Slot::Accumulator, 0);
                    cpu.set_wide(Slot::Base, 0);
                }
                return None;
            }
            if v == 0x2f {
                multiplex(cpu);
                return None;
            }
            return handle_event(Outcome::Interrupt(v), cpu.instruction(), cpu, ram, session);
        }
        Outcome::Continue => {}
        Outcome::EmsReturn => session.ems.return_call(cpu, ram),
        Outcome::Ems => {
            if step::interrupt_return(cpu, ram, 0xcf) != Outcome::Continue {
                return Some(End::Fault(0xcf));
            }
            session.ems.call(cpu, ram);
        }
        Outcome::Interrupt(0x67) if !session.vector_live[0x67] => session.ems.call(cpu, ram),
        Outcome::Xms => {
            session.xms.call(cpu, ram, &mut session.arena);
            if !step::far_return(cpu, ram) {
                return Some(End::Fault(0xcb));
            }
        }
        Outcome::Port { port, write, word } => serve_port(cpu, ram, port, write, word),
        Outcome::Interrupt(0x2E) => super::command::invoke(cpu, ram, session),
        Outcome::Interrupt(0x21) if matches!(cpu.high(Slot::Accumulator), 1 | 7 | 8) => {
            if let Some(key) = crate::bios::pc::key(true) {
                cpu.set_low(Slot::Accumulator, key as u8);
                if cpu.high(Slot::Accumulator) == 1 {
                    crate::bios::mscon::con_write_bytes(&[key as u8]);
                }
            } else {
                cpu.set_instruction(instruction);
                if session.scheduled {
                    return Some(End::Waiting);
                }
            }
        }
        Outcome::Interrupt(0x21) => match service(cpu, ram, session) {
            Action::Continue => {}
            Action::Exit(code) => return termination::exit(cpu, ram, session, code),
        },
        Outcome::Interrupt(v @ (0x25 | 0x26)) => drivers::absolute(cpu, ram, session, v == 0x26),
        Outcome::Interrupt(0x03) => {}
        Outcome::Interrupt(0x20) => return termination::exit(cpu, ram, session, 0),
        Outcome::Halt => return Some(End::Stopped(0)),
        Outcome::Fault(opcode) => {
            cpu.set_instruction(instruction);
            return Some(End::Fault(opcode));
        }
        Outcome::Interrupt(0x16)
            if matches!(cpu.high(Slot::Accumulator), 0 | 0x10)
                && crate::bios::pc::key(false).is_none() =>
        {
            cpu.set_instruction(instruction);
            if session.scheduled {
                return Some(End::Waiting);
            }
        }
        Outcome::Interrupt(vector) => {
            if serve_bios(cpu, ram, vector) {
            } else if !enter_vector(cpu, ram, session, vector) {
                // An unclaimed multiplex call chains to the final IRET. Probes
                // use AL=0 on entry and observe that no TSR claimed it.
                if vector == 0x2f {
                    multiplex(cpu);
                    return None;
                }
                return Some(End::Other(vector));
            }
        }
    }
    None
}

fn multiplex(cpu: &mut CpuState) {
    match cpu.wide(Slot::Accumulator) {
        0x4300 => cpu.set_low(Slot::Accumulator, 0x80),
        0x4310 => {
            cpu.set_selector(Segment::Extra, xms::ENTRY.0);
            cpu.set_wide(Slot::Base, xms::ENTRY.1);
        }
        _ => {}
    }
}

enum Action {
    Continue,
    Exit(u8),
}

#[derive(Clone, Copy)]
struct Opened {
    device: Option<drivers::Device>,
    dir: u32,
    pos: usize,
    name: [u8; 11],
    named: bool,
    console: bool,
    access: u8,
    dirty: bool,
}

const HANDLE_SLOTS: usize = 20;

// A single INT 21h transfer fits in CX. Open descriptions carry only metadata.
static TRANSFER: KernelCell<[u8; u16::MAX as usize]> = KernelCell::new([0; u16::MAX as usize]);

pub(super) struct Session {
    drivers: drivers::Drivers,
    terminal: usize,
    pub(super) scheduled: bool,
    pending: Option<CpuState>,
    parents: [Option<Parent>; 8],
    pub(super) batches: super::command::Batches,
    pub(super) environment: super::command::Environment,
    handles: handles::Handles,
    xms: xms::Xms,
    ems: ems::Ems,
    last_code: u16,
    dta_segment: u16,
    dta_offset: u16,
    prefix: u16,
    arena: Arena,
    pub(super) child_depth: u8,
    pub(super) drive: u8,
    break_check: u8,
    extended_break: u8,
    current_psp: u16,
    extended: u16,
    extended_class: u8,
    extended_action: u8,
    extended_locus: u8,
    vector_segment: [u16; 256],
    vector_offset: [u16; 256],
    vector_live: [bool; 256],
    verify: u8,
    strategy: u8,
    code_page: u16,
    switch_char: u8,
    machine_name: [u8; 15],
    machine_defined: bool,
    redirects: [Redirect; 4],
    year: u16,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
    hundredths: u8,
}

impl Session {
    fn new() -> Self {
        Self {
            drivers: drivers::Drivers::new(),
            terminal: 0,
            scheduled: false,
            pending: None,
            parents: [const { None }; 8],
            batches: super::command::Batches::new(),
            environment: super::command::Environment::new(),
            handles: handles::Handles::new(),
            xms: xms::Xms::new(),
            ems: ems::Ems::new(),
            last_code: 0,
            dta_segment: 0,
            dta_offset: 0,
            prefix: 0,
            arena: Arena::new(),
            child_depth: 0,
            drive: 0,
            break_check: 0,
            extended_break: 0,
            current_psp: 0,
            extended: 0,
            extended_class: 0,
            extended_action: 0,
            extended_locus: 0,
            vector_segment: [0; 256],
            vector_offset: [0; 256],
            vector_live: [false; 256],
            verify: 0,
            strategy: 0,
            code_page: 437,
            switch_char: b'/',
            machine_name: [b' '; 15],
            machine_defined: false,
            redirects: [Redirect::empty(); 4],
            year: 1980,
            month: 1,
            day: 1,
            weekday: 2,
            hour: 0,
            minute: 0,
            second: 0,
            hundredths: 0,
        }
    }
}

fn direct_console(cpu: &mut CpuState) {
    let value = cpu.low(Slot::Data);
    if value == 0xFF {
        let key = crate::bios::pc::key(true);
        cpu.set_low(Slot::Accumulator, key.unwrap_or(0) as u8);
        cpu.set_zero(key.is_none());
        cpu.set_carry(false);
        return;
    }
    crate::bios::mscon::con_write_bytes(&[value]);
    cpu.set_carry(false);
}

fn service(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) -> Action {
    let function = cpu.high(Slot::Accumulator);
    if !matches!(function, 0x50 | 0x51 | 0x62) && session.current_psp != 0 {
        termination::save_stack(cpu, ram, session.current_psp);
    }
    let result = dispatch(cpu, ram, session);
    if function == 0x46 {
        locks::discard_closed(session);
    }
    if matches!(function, 0x48..=0x4a) {
        session.arena.publish(ram);
    }
    if matches!(
        function,
        0x3c..=0x40 | 0x42 | 0x45 | 0x46 | 0x5a | 0x5b | 0x67 | 0x6c
    ) {
        session.handles.publish(ram, session.prefix);
    }
    result
}
fn dispatch(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) -> Action {
    // A registered block unit currently exposes raw sectors. Until it has a
    // filesystem mount, never let a path on that unit mutate the boot volume.
    let function = cpu.high(Slot::Accumulator);
    if matches!(
        function,
        0x39..=0x3d | 0x41 | 0x43 | 0x4b | 0x4e | 0x56 | 0x5a | 0x5b | 0x6c
    ) {
        let raw_path = |seg, off: u16| {
            ram.load(seg, off.wrapping_add(1)) == Some(b':')
                && ram
                    .load(seg, off)
                    .is_some_and(|letter| session.drivers.is_raw_volume(letter))
        };
        if raw_path(
            cpu.selector(Segment::Data),
            cpu.wide(if function == 0x6c {
                Slot::Source
            } else {
                Slot::Data
            }),
        ) || function == 0x56 && raw_path(cpu.selector(Segment::Extra), cpu.wide(Slot::Dest))
        {
            fail(cpu, session, 15);
            return Action::Continue;
        }
    }
    match cpu.high(Slot::Accumulator) {
        0x0B => {
            cpu.set_low(
                Slot::Accumulator,
                if crate::bios::pc::key(false).is_some() {
                    0xff
                } else {
                    0
                },
            );
            cpu.set_carry(false);
            Action::Continue
        }
        0x0D => {
            disk_reset(cpu, session);
            Action::Continue
        }
        0x06 => {
            direct_console(cpu);
            Action::Continue
        }
        0x05 => {
            let _ = crate::bios::mslpt::prn_write_bytes(&[cpu.low(Slot::Data)]);
            cpu.set_carry(false);
            Action::Continue
        }
        0x04 => {
            crate::bios::msaux::aux_write_bytes(&[cpu.low(Slot::Data)]);
            cpu.set_carry(false);
            Action::Continue
        }
        0x02 => {
            crate::bios::mscon::con_write_bytes(&[cpu.low(Slot::Data)]);
            Action::Continue
        }
        0x09 => {
            print_marked(cpu, ram);
            Action::Continue
        }
        0x3D => {
            open_file(cpu, ram, session);
            Action::Continue
        }
        0x3E => {
            close_file(cpu, session);
            Action::Continue
        }
        0x3F => {
            read_file(cpu, ram, session);
            Action::Continue
        }
        0x40 => {
            write_output(cpu, ram, session);
            Action::Continue
        }
        0x4B => {
            launch(cpu, ram, session);
            Action::Continue
        }
        0x19 => {
            cpu.set_low(Slot::Accumulator, session.drive);
            cpu.set_carry(false);
            Action::Continue
        }
        0x0E => {
            let requested = cpu.low(Slot::Data);
            if super::disk::select_drive(requested) {
                session.drive = requested;
            }
            cpu.set_low(Slot::Accumulator, session.drivers.count());
            cpu.set_carry(false);
            Action::Continue
        }
        0x29 => {
            parse_filename(cpu, ram);
            Action::Continue
        }
        0x33 => {
            system_values(cpu, session);
            Action::Continue
        }
        0x34 => {
            indos_flag(cpu, ram);
            Action::Continue
        }
        0x46 => {
            force_duplicate(cpu, session);
            Action::Continue
        }
        0x59 => {
            extended_error(cpu, session);
            Action::Continue
        }
        0x62 => {
            current_process(cpu, session);
            Action::Continue
        }
        0x66 => {
            code_page(cpu, session);
            Action::Continue
        }
        0x65 => {
            uppercase_info(cpu, ram, session);
            Action::Continue
        }
        0x1B => {
            cpu.set_low(Slot::Data, 0);
            drive_info(cpu, ram);
            Action::Continue
        }
        0x1C => {
            drive_info(cpu, ram);
            Action::Continue
        }
        0x1A => {
            session.dta_segment = cpu.selector(Segment::Data);
            session.dta_offset = cpu.wide(Slot::Data);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2F => {
            let (segment, offset) = dta_target(session);
            cpu.set_selector(Segment::Extra, segment);
            cpu.set_wide(Slot::Base, offset);
            cpu.set_carry(false);
            Action::Continue
        }
        0x0F => {
            fcb_open(cpu, ram);
            Action::Continue
        }
        0x10 => {
            fcb_close(cpu, ram);
            Action::Continue
        }
        0x11 => {
            fcb_find(cpu, ram, session, false);
            Action::Continue
        }
        0x12 => {
            fcb_find(cpu, ram, session, true);
            Action::Continue
        }
        0x13 => {
            fcb_delete(cpu, ram);
            Action::Continue
        }
        0x14 => {
            fcb_read(cpu, ram, session);
            Action::Continue
        }
        0x15 => {
            fcb_write(cpu, ram, session);
            Action::Continue
        }
        0x16 => {
            fcb_create(cpu, ram);
            Action::Continue
        }
        0x17 => {
            fcb_rename(cpu, ram);
            Action::Continue
        }
        0x21 => {
            fcb_random_read(cpu, ram, session);
            Action::Continue
        }
        0x22 => {
            fcb_random_write(cpu, ram, session);
            Action::Continue
        }
        0x23 => {
            fcb_size(cpu, ram);
            Action::Continue
        }
        0x24 => {
            fcb_place(cpu, ram);
            Action::Continue
        }
        0x27 => {
            fcb_block_read(cpu, ram, session);
            Action::Continue
        }
        0x28 => {
            fcb_block_write(cpu, ram, session);
            Action::Continue
        }
        0x25 => {
            termination::vector(
                session,
                ram,
                cpu.low(Slot::Accumulator),
                (cpu.selector(Segment::Data), cpu.wide(Slot::Data)),
            );
            cpu.set_carry(false);
            Action::Continue
        }
        0x26 => {
            create_psp(cpu, ram, session);
            Action::Continue
        }
        0x2A => {
            cpu.set_wide(Slot::Count, session.year);
            cpu.set_high(Slot::Data, session.month);
            cpu.set_low(Slot::Data, session.day);
            cpu.set_low(Slot::Accumulator, session.weekday);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2B => {
            set_clock_date(cpu, session);
            Action::Continue
        }
        0x2E => {
            session.verify = u8::from(cpu.low(Slot::Accumulator) != 0);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2C => {
            cpu.set_high(Slot::Count, session.hour);
            cpu.set_low(Slot::Count, session.minute);
            cpu.set_high(Slot::Data, session.second);
            cpu.set_low(Slot::Data, session.hundredths);
            cpu.set_carry(false);
            Action::Continue
        }
        0x2D => {
            set_clock_time(cpu, session);
            Action::Continue
        }
        0x30 => {
            let (default, bx, cx) = crate::dos::getset::VERSION;
            let version = read_word(ram, session.current_psp, 0x40)
                .filter(|&v| v != 0)
                .unwrap_or(default);
            cpu.set_wide(Slot::Accumulator, version);
            cpu.set_wide(Slot::Base, bx);
            cpu.set_wide(Slot::Count, cx);
            Action::Continue
        }
        0x32 => {
            drive_parameters(cpu, ram);
            Action::Continue
        }
        0x35 => {
            let id = usize::from(cpu.low(Slot::Accumulator));
            cpu.set_selector(
                Segment::Extra,
                read_word(ram, 0, id as u16 * 4 + 2).unwrap(),
            );
            cpu.set_wide(Slot::Base, read_word(ram, 0, id as u16 * 4).unwrap());
            cpu.set_carry(false);
            Action::Continue
        }
        0x36 => {
            report_space(cpu);
            Action::Continue
        }
        0x37 => {
            switch_character(cpu, session);
            Action::Continue
        }
        0x38 => {
            country_info(cpu, ram, session);
            Action::Continue
        }
        0x39 => {
            cpu.set_carry(!named(cpu, ram, super::disk::make_dir));
            Action::Continue
        }
        0x3A => {
            cpu.set_carry(!named(cpu, ram, super::disk::remove_dir));
            Action::Continue
        }
        0x3B => {
            cpu.set_carry(!change_directory(cpu, ram));
            Action::Continue
        }
        0x3C => {
            create_file(cpu, ram, session);
            Action::Continue
        }
        0x41 => {
            cpu.set_carry(!named(cpu, ram, super::disk::remove_file));
            Action::Continue
        }
        0x42 => {
            seek_file(cpu, session);
            Action::Continue
        }
        0x43 => {
            read_attribute(cpu, ram);
            Action::Continue
        }
        0x47 => {
            current_directory(cpu, ram, session);
            Action::Continue
        }
        0x44 => {
            if !drivers::ioctl(cpu, ram, session) {
                device_control(cpu, session);
            }
            Action::Continue
        }
        0x63 => {
            if cpu.low(Slot::Accumulator) == 0
                && ram.store(DBCS_SEGMENT, 0, 0)
                && ram.store(DBCS_SEGMENT, 1, 0)
            {
                cpu.set_selector(Segment::Data, DBCS_SEGMENT);
                cpu.set_wide(Slot::Source, 0);
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
            Action::Continue
        }
        0x45 => {
            duplicate(cpu, session);
            Action::Continue
        }
        0x48 => {
            let result =
                session
                    .arena
                    .alloc(session.prefix, cpu.wide(Slot::Base), session.strategy);
            if let Ok(start) = result {
                cpu.set_wide(Slot::Accumulator, start);
            }
            memory_status(cpu, session, result.map(|_| ()));
            Action::Continue
        }
        0x49 => {
            let result = session
                .arena
                .free(session.prefix, cpu.selector(Segment::Extra));
            memory_status(cpu, session, result);
            Action::Continue
        }
        0x4A => {
            let start = cpu.selector(Segment::Extra);
            let result = session
                .arena
                .resize(session.prefix, start, cpu.wide(Slot::Base));
            if let Ok(end) = result
                && start == session.prefix
            {
                let bytes = end.to_le_bytes();
                ram.store(start, 2, bytes[0]);
                ram.store(start, 3, bytes[1]);
            }
            memory_status(cpu, session, result.map(|_| ()));
            Action::Continue
        }
        0x4C => Action::Exit(cpu.low(Slot::Accumulator)),
        0x50 => {
            session.current_psp = cpu.wide(Slot::Base);
            cpu.set_carry(false);
            Action::Continue
        }
        0x51 => {
            current_process(cpu, session);
            Action::Continue
        }
        0x58 => {
            allocation_strategy(cpu, ram, session);
            Action::Continue
        }
        0x4E => {
            find_first(cpu, ram, session);
            Action::Continue
        }
        0x4F => {
            find_next(cpu, ram, session);
            Action::Continue
        }
        0x52 => {
            list_of_lists(cpu, ram, session);
            Action::Continue
        }
        0x55 => {
            let destination = cpu.wide(Slot::Data);
            create_psp(cpu, ram, session);
            if cpu.flags() & 1 == 0 {
                let limit = cpu.wide(Slot::Source);
                store_bytes(ram, destination, 2, &limit.to_le_bytes());
                store_bytes(ram, destination, 0x34, &[0x18, 0]);
                store_bytes(ram, destination, 0x36, &destination.to_le_bytes());
                session.current_psp = destination;
                cpu.set_low(Slot::Accumulator, 0xf0);
            }
            Action::Continue
        }
        0x54 => {
            cpu.set_low(Slot::Accumulator, session.verify);
            cpu.set_carry(false);
            Action::Continue
        }
        0x56 => {
            rename_path(cpu, ram);
            Action::Continue
        }
        0x57 => {
            file_time(cpu, session);
            Action::Continue
        }
        0x5A => {
            create_temp(cpu, ram, session);
            Action::Continue
        }
        0x5E => {
            machine_name(cpu, ram, session);
            Action::Continue
        }
        0x5F => {
            redirect_list(cpu, ram, session);
            Action::Continue
        }
        0x5B => {
            create_new(cpu, ram, session);
            Action::Continue
        }
        0x5C => {
            locks::service(cpu, session);
            Action::Continue
        }
        0x6C => {
            extended_open(cpu, ram, session);
            Action::Continue
        }
        0x60 => {
            truename(cpu, ram, session);
            Action::Continue
        }
        0x68 => {
            commit_file(cpu, session);
            Action::Continue
        }
        0x69 => {
            volume_serial(cpu, ram, session);
            Action::Continue
        }
        0x67 => {
            set_handle_count(cpu, session);
            Action::Continue
        }
        0x4D => {
            cpu.set_wide(Slot::Accumulator, session.last_code);
            session.last_code = 0;
            cpu.set_carry(false);
            Action::Continue
        }
        _ => {
            fail(cpu, session, 1);
            Action::Continue
        }
    }
}

const ERROR_MAP: [(u16, u8, u8, u8); 31] = [
    (1, 7, 4, 0xFF),
    (2, 8, 3, 2),
    (3, 8, 3, 2),
    (4, 1, 4, 1),
    (5, 3, 3, 0xFF),
    (6, 7, 4, 1),
    (7, 7, 5, 5),
    (8, 1, 4, 5),
    (9, 7, 4, 5),
    (10, 7, 4, 5),
    (11, 9, 3, 1),
    (12, 7, 4, 1),
    (13, 9, 4, 1),
    (15, 8, 3, 2),
    (16, 3, 3, 2),
    (17, 13, 3, 2),
    (18, 8, 3, 2),
    (32, 10, 2, 2),
    (33, 10, 2, 2),
    (36, 1, 4, 5),
    (38, 1, 4, 1),
    (39, 1, 4, 1),
    (50, 9, 3, 3),
    (80, 12, 3, 2),
    (82, 1, 4, 2),
    (83, 13, 4, 1),
    (84, 1, 4, 0xFF),
    (85, 12, 3, 3),
    (86, 3, 3, 1),
    (87, 9, 3, 1),
    (90, 13, 4, 2),
];

fn note_error(session: &mut Session, code: u16) {
    session.extended = code;
    for (number, class, action, locus) in ERROR_MAP {
        if number == code {
            session.extended_class = class;
            session.extended_action = action;
            if locus != 0xFF {
                session.extended_locus = locus;
            }
            return;
        }
    }
}

fn fail(cpu: &mut CpuState, session: &mut Session, code: u16) {
    note_error(session, code);
    cpu.set_wide(Slot::Accumulator, code);
    cpu.set_carry(true);
}

fn extended_error(cpu: &mut CpuState, session: &Session) {
    cpu.set_wide(Slot::Accumulator, session.extended);
    cpu.set_wide(
        Slot::Base,
        (u16::from(session.extended_class) << 8) | u16::from(session.extended_action),
    );
    let kept = cpu.wide(Slot::Count) & 0x00FF;
    cpu.set_wide(Slot::Count, kept | (u16::from(session.extended_locus) << 8));
    cpu.set_selector(Segment::Extra, 0);
    cpu.set_wide(Slot::Dest, 0);
    cpu.set_carry(false);
}

fn allocation_strategy(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        0 => {
            cpu.set_wide(Slot::Accumulator, u16::from(session.strategy));
            cpu.set_carry(false);
        }
        1 => {
            let requested = cpu.wide(Slot::Base);
            if !matches!(requested, 0..=2 | 0x40..=0x42 | 0x80..=0x82) {
                fail(cpu, session, 1);
                return;
            }
            session.strategy = requested as u8;
            cpu.set_carry(false);
        }
        2 => {
            cpu.set_low(Slot::Accumulator, u8::from(session.arena.upper_linked));
            cpu.set_carry(false);
        }
        3 if cpu.wide(Slot::Base) <= 1 => {
            session.arena.upper_linked = cpu.wide(Slot::Base) != 0;
            session.arena.publish(ram);
            cpu.set_carry(false);
        }
        _ => fail(cpu, session, 1),
    }
}

#[derive(Clone, Copy)]
struct Redirect {
    local: [u8; 16],
    remote: [u8; 32],
    kind: u8,
    parameter: u16,
    live: bool,
}

impl Redirect {
    const fn empty() -> Self {
        Self {
            local: [0; 16],
            remote: [0; 32],
            kind: 0,
            parameter: 0,
            live: false,
        }
    }
}

fn redirect_list(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        2 => redirect_get(cpu, ram, session),
        3 => redirect_set(cpu, ram, session),
        _ => fail(cpu, session, 1),
    }
}

fn redirect_set(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let mut local = [0u8; 16];
    let mut remote = [0u8; 32];
    if !read_text(
        ram,
        cpu.selector(Segment::Data),
        cpu.wide(Slot::Source),
        &mut local,
    ) || !read_text(
        ram,
        cpu.selector(Segment::Extra),
        cpu.wide(Slot::Dest),
        &mut remote,
    ) {
        fail(cpu, session, 5);
        return;
    }
    let mut slot = 0usize;
    let mut found = None;
    while slot < session.redirects.len() {
        if session.redirects[slot].live && session.redirects[slot].local == local {
            found = Some(slot);
            break;
        }
        slot += 1;
    }
    if found.is_none() {
        slot = 0;
        while slot < session.redirects.len() {
            if !session.redirects[slot].live {
                found = Some(slot);
                break;
            }
            slot += 1;
        }
    }
    let Some(slot) = found else {
        fail(cpu, session, 8);
        return;
    };
    session.redirects[slot] = Redirect {
        local,
        remote,
        kind: cpu.low(Slot::Base),
        parameter: cpu.wide(Slot::Count),
        live: true,
    };
    cpu.set_carry(false);
}

fn redirect_get(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let Some(slot) = redirect_at(session, cpu.wide(Slot::Base)) else {
        fail(cpu, session, 0x12);
        return;
    };
    let entry = session.redirects[slot];
    if !write_text(
        ram,
        cpu.selector(Segment::Data),
        cpu.wide(Slot::Source),
        &entry.local,
    ) || !write_text(
        ram,
        cpu.selector(Segment::Extra),
        cpu.wide(Slot::Dest),
        &entry.remote,
    ) {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_high(Slot::Base, 0);
    cpu.set_low(Slot::Base, entry.kind);
    cpu.set_wide(Slot::Count, entry.parameter);
    cpu.set_carry(false);
}

fn redirect_at(session: &Session, index: u16) -> Option<usize> {
    let mut seen = 0u16;
    let mut slot = 0usize;
    while slot < session.redirects.len() {
        if session.redirects[slot].live {
            if seen == index {
                return Some(slot);
            }
            seen = seen.saturating_add(1);
        }
        slot += 1;
    }
    None
}

fn read_text(ram: &GuestRam, segment: u16, mut offset: u16, out: &mut [u8]) -> bool {
    let mut index = 0usize;
    while index < out.len() {
        let Some(byte) = ram.load(segment, offset) else {
            return false;
        };
        out[index] = byte;
        index += 1;
        offset = offset.wrapping_add(1);
        if byte == 0 {
            while index < out.len() {
                out[index] = 0;
                index += 1;
            }
            return true;
        }
    }
    false
}

fn write_text(ram: &mut GuestRam, segment: u16, mut offset: u16, text: &[u8]) -> bool {
    for byte in text {
        if !ram.store(segment, offset, *byte) {
            return false;
        }
        offset = offset.wrapping_add(1);
        if *byte == 0 {
            return true;
        }
    }
    false
}

fn machine_name(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let segment = cpu.selector(Segment::Data);
    let start = cpu.wide(Slot::Data);
    match cpu.low(Slot::Accumulator) {
        0 => {
            if session.machine_defined {
                let mut offset = start;
                for byte in session.machine_name {
                    if !ram.store(segment, offset, byte) {
                        fail(cpu, session, 5);
                        return;
                    }
                    offset = offset.wrapping_add(1);
                }
                cpu.set_high(Slot::Count, 1);
            } else {
                cpu.set_high(Slot::Count, 0);
            }
            cpu.set_low(Slot::Count, 0);
            cpu.set_carry(false);
        }
        1 => {
            if cpu.high(Slot::Count) == 0 {
                session.machine_defined = false;
                cpu.set_carry(false);
                return;
            }
            let mut name = [b' '; 15];
            if !load_bytes(ram, segment, start, &mut name) {
                fail(cpu, session, 5);
                return;
            }
            session.machine_name = name;
            session.machine_defined = true;
            cpu.set_carry(false);
        }
        _ => fail(cpu, session, 1),
    }
}

fn set_clock_time(cpu: &mut CpuState, session: &mut Session) {
    let hour = cpu.high(Slot::Count);
    let minute = cpu.low(Slot::Count);
    let second = cpu.high(Slot::Data);
    let hundredths = cpu.low(Slot::Data);
    match crate::dos::time::set_time(hour, minute, second, hundredths) {
        Ok(()) => {
            session.hour = hour;
            session.minute = minute;
            session.second = second;
            session.hundredths = hundredths;
            cpu.set_low(Slot::Accumulator, 0);
            cpu.set_carry(false);
        }
        Err(_) => {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_carry(true);
        }
    }
}

fn set_clock_date(cpu: &mut CpuState, session: &mut Session) {
    let year = cpu.wide(Slot::Count);
    let month = cpu.high(Slot::Data);
    let day = cpu.low(Slot::Data);
    match crate::dos::time::set_date(year, month, day) {
        Ok(date) => {
            session.year = date.year;
            session.month = date.month;
            session.day = date.day;
            session.weekday = date.weekday;
            cpu.set_low(Slot::Accumulator, 0);
            cpu.set_carry(false);
        }
        Err(_) => {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_carry(true);
        }
    }
}

fn create_psp(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let source = session.current_psp;
    let dest = cpu.wide(Slot::Data);
    if source == 0 || dest == 0 {
        cpu.set_carry(true);
        return;
    }
    let mut offset = 0u16;
    while offset < 0x100 {
        let Some(byte) = ram.load(source, offset) else {
            cpu.set_carry(true);
            return;
        };
        if !ram.store(dest, offset, byte) {
            cpu.set_carry(true);
            return;
        }
        offset = offset.wrapping_add(1);
    }
    let parent = source.to_le_bytes();
    if ram.store(dest, 0, 0xCD)
        && ram.store(dest, 1, 0x20)
        && ram.store(dest, 0x16, parent[0])
        && ram.store(dest, 0x17, parent[1])
    {
        termination::save_vectors(ram, dest);
        cpu.set_carry(false);
    } else {
        cpu.set_carry(true);
    }
}

fn current_process(cpu: &mut CpuState, session: &Session) {
    cpu.set_wide(Slot::Base, session.current_psp);
    cpu.set_carry(false);
}

fn console_handle(handle: u16) -> Opened {
    Opened {
        device: None,
        dir: 0,
        pos: 0,
        name: *b"CON        ",
        named: false,
        console: true,
        access: if handle == 0 { 0 } else { 1 },
        dirty: false,
    }
}

fn copy_handle(session: &Session, handle: u16) -> Option<Opened> {
    session.handles.get(handle).copied()
}
fn force_duplicate(cpu: &mut CpuState, session: &mut Session) {
    match session
        .handles
        .duplicate(cpu.wide(Slot::Base), Some(cpu.wide(Slot::Count)))
    {
        Ok(_) => cpu.set_carry(false),
        Err(code) => fail(cpu, session, code),
    }
}
fn disk_reset(cpu: &mut CpuState, session: &mut Session) {
    if session.handles.commit_all() && super::disk::drive_root(2).is_none_or(super::disk::flush) {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn open_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let access = cpu.low(Slot::Accumulator) & 7;
    if access > 2 {
        fail(cpu, session, 12);
        return;
    }
    let Some((dir, name)) = search_spec(&guest_text(cpu, ram)) else {
        fail(cpu, session, 2);
        return;
    };
    match open_named(
        session,
        dir,
        name,
        access | cpu.low(Slot::Accumulator) & 0x80,
        false,
    ) {
        Ok(handle) => {
            cpu.set_wide(Slot::Accumulator, handle);
            cpu.set_carry(false);
        }
        Err(code) => fail(cpu, session, code),
    }
}

fn flush_opened(opened: &Opened) -> bool {
    !opened.named || super::disk::flush(opened.dir)
}

fn commit_file(cpu: &mut CpuState, session: &mut Session) {
    let Some(opened) = session.handles.get_mut(cpu.wide(Slot::Base)) else {
        fail(cpu, session, 6);
        return;
    };
    if flush_opened(opened) {
        opened.dirty = false;
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn close_file(cpu: &mut CpuState, session: &mut Session) {
    let h = cpu.wide(Slot::Base);
    match session.handles.close(h) {
        Ok(()) => {
            locks::discard_closed(session);
            cpu.set_carry(false);
        }
        Err(code) => fail(cpu, session, code),
    }
}

fn read_file(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if drivers::handle_io(cpu, ram, session, false) {
        return;
    }
    let handle = cpu.wide(Slot::Base);
    let requested = usize::from(cpu.wide(Slot::Count));
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
    let Some(found) = copy_handle(session, handle) else {
        fail(cpu, session, 6);
        return;
    };
    if found.console {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
        return;
    }
    if found.access == 1 {
        fail(cpu, session, 5);
        return;
    }
    if requested > 0 {
        let start = u32::try_from(found.pos).unwrap_or(u32::MAX);
        let span = u32::try_from(requested).unwrap_or(u32::MAX);
        if locks::blocked(session, handle, start, span) {
            fail(cpu, session, 0x21);
            return;
        }
    }
    let Some(opened) = session.handles.get_mut(handle) else {
        fail(cpu, session, 6);
        return;
    };
    let mut transfer = TRANSFER.borrow();
    let Some(n) = super::disk::read_range(
        opened.dir,
        &opened.name,
        opened.pos,
        &mut transfer[..requested],
    ) else {
        fail(cpu, session, 5);
        return;
    };
    let limit = if &opened.name[8..] == b"SKL" {
        transfer[..n]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(n, |i| i + 1)
    } else {
        n
    };
    let mut moved = 0;
    for &byte in &transfer[..limit] {
        if !ram.store(segment, offset, byte) {
            break;
        }
        offset = offset.wrapping_add(1);
        moved += 1;
    }
    opened.pos += moved;
    cpu.set_wide(Slot::Accumulator, moved as u16);
    cpu.set_carry(false);
}

fn create_temp(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let segment = cpu.selector(Segment::Data);
    let start = cpu.wide(Slot::Data);
    let text = bytes_at(ram, segment, start);
    let mut length = 0usize;
    while length < text.len() && text[length] != 0 {
        length += 1;
    }
    if length == 0 || length == text.len() {
        fail(cpu, session, 3);
        return;
    }
    if session.handles.available().is_none() {
        fail(cpu, session, 4);
        return;
    }
    let path = text[..length].trim_ascii_start();
    if !path.last().is_some_and(|b| matches!(*b, b'/' | 92)) {
        fail(cpu, session, 3);
        return;
    }
    if super::disk::path_start(path).is_none() {
        fail(cpu, session, 15);
        return;
    }
    let Some(dir) = super::disk::resolve_dir(path) else {
        fail(cpu, session, 3);
        return;
    };
    let attr = (cpu.wide(Slot::Count) as u8) & 0x27;
    let mut name = [b' '; 11];
    let Some(chosen) = (0..=0xffff).find_map(|serial| {
        hex_name(serial, &mut name);
        (!super::disk::exists_at(dir, &name)).then_some(name)
    }) else {
        fail(cpu, session, 4);
        return;
    };
    if !super::disk::write_named_at(dir, &chosen, b"", attr) {
        fail(cpu, session, 5);
        return;
    }
    let Ok(handle) = open_named(session, dir, chosen, 2, true) else {
        fail(cpu, session, 4);
        return;
    };
    let mut index = 0usize;
    let mut offset = start.wrapping_add(length as u16);
    while index < 8 {
        if !ram.store(segment, offset, chosen[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if !ram.store(segment, offset, 0) {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_wide(Slot::Accumulator, handle);
    cpu.set_carry(false);
}

fn hex_name(serial: u32, name: &mut [u8; 11]) {
    let digits = b"0123456789ABCDEF";
    let mut index = 0usize;
    while index < 8 {
        let shift = (7 - index) * 4;
        name[index] = digits[((serial >> shift) & 0xF) as usize];
        index += 1;
    }
    name[8] = b' ';
    name[9] = b' ';
    name[10] = b' ';
}

fn create_new(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    if let Some((dir, name)) = search_spec(&guest_text(cpu, ram))
        && (session.drivers.find(&name).is_some() || super::disk::exists_at(dir, &name))
    {
        fail(cpu, session, 0x50);
        return;
    }
    create_file(cpu, ram, session);
}

fn extended_open(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        fail(cpu, session, 1);
        return;
    }
    let action = cpu.wide(Slot::Data);
    let access = cpu.low(Slot::Base) & 7;
    if access > 2 {
        fail(cpu, session, 12);
        return;
    }
    let attr = (cpu.wide(Slot::Count) as u8) & 0x27;
    let Some((dir, name)) = search_spec(&bytes_at(
        ram,
        cpu.selector(Segment::Data),
        cpu.wide(Slot::Source),
    )) else {
        fail(cpu, session, 2);
        return;
    };
    if session.handles.available().is_none() {
        fail(cpu, session, 4);
        return;
    }
    let device = session.drivers.find(&name).is_some();
    let present = device || super::disk::exists_at(dir, &name);
    let exist_action = action & 0x0F;
    let missing_action = action >> 4;
    let (empty, taken) = if present && exist_action == 1 {
        (false, 1u16)
    } else if present && exist_action == 2 {
        if !device && !super::disk::write_named_at(dir, &name, b"", attr) {
            fail(cpu, session, 5);
            return;
        }
        (true, 3u16)
    } else if present {
        fail(cpu, session, 0x50);
        return;
    } else if missing_action == 1 {
        if !super::disk::write_named_at(dir, &name, b"", attr) {
            fail(cpu, session, 5);
            return;
        }
        (true, 2u16)
    } else {
        fail(cpu, session, 2);
        return;
    };
    let handle = match open_named(
        session,
        dir,
        name,
        access | cpu.low(Slot::Base) & 0x80,
        empty,
    ) {
        Ok(handle) => handle,
        Err(code) => {
            fail(cpu, session, code);
            return;
        }
    };
    cpu.set_wide(Slot::Accumulator, handle);
    cpu.set_wide(Slot::Count, taken);
    cpu.set_carry(false);
}

fn open_named(
    session: &mut Session,
    dir: u32,
    name: [u8; 11],
    access: u8,
    empty: bool,
) -> Result<u16, u16> {
    let (handle, slot) = session.handles.available().ok_or(4u16)?;
    let inherit = access & 0x80 == 0;
    let access = access & 7;
    let file = if let Some(device) = session.drivers.find(&name) {
        Opened {
            device: Some(device),
            name,
            access,
            ..console_handle(0)
        }
    } else {
        if !empty {
            super::disk::file_len_at(dir, &name).ok_or(2u16)?;
        }
        Opened {
            device: None,
            dir,
            pos: 0,
            name,
            named: true,
            console: false,
            access,
            dirty: false,
        }
    };
    session.handles.insert(handle, slot, file, inherit);
    Ok(handle as u16)
}
fn create_file(cpu: &mut CpuState, ram: &GuestRam, session: &mut Session) {
    let Some((dir, name)) = search_spec(&guest_text(cpu, ram)) else {
        fail(cpu, session, 2);
        return;
    };
    if session.handles.available().is_none() {
        fail(cpu, session, 4);
        return;
    }
    if session.drivers.find(&name).is_none()
        && !super::disk::write_named_at(dir, &name, b"", cpu.low(Slot::Count) & 0x27)
    {
        fail(cpu, session, 0x27);
        return;
    }
    match open_named(session, dir, name, 2, true) {
        Ok(h) => {
            cpu.set_wide(Slot::Accumulator, h);
            cpu.set_carry(false);
        }
        Err(code) => fail(cpu, session, code),
    }
}
fn duplicate(cpu: &mut CpuState, session: &mut Session) {
    match session.handles.duplicate(cpu.wide(Slot::Base), None) {
        Ok(h) => {
            cpu.set_wide(Slot::Accumulator, h);
            cpu.set_carry(false);
        }
        Err(code) => fail(cpu, session, code),
    }
}
fn set_handle_count(cpu: &mut CpuState, session: &mut Session) {
    let requested = cpu.wide(Slot::Base);
    if requested < session.handles.span() {
        fail(cpu, session, 4);
        return;
    }
    if requested > HANDLE_SLOTS as u16 {
        fail(cpu, session, 8);
        return;
    }
    session.handles.job.limit = requested;
    cpu.set_carry(false);
}

fn write_output(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if drivers::handle_io(cpu, ram, session, true) {
        return;
    }
    let handle = cpu.wide(Slot::Base);
    let Some(found) = copy_handle(session, handle) else {
        fail(cpu, session, 6);
        return;
    };
    if found.console {
        write_console(cpu, ram);
        return;
    }
    if !found.named || found.access == 0 {
        fail(cpu, session, 5);
        return;
    }
    let count = cpu.wide(Slot::Count);
    if count > 0 {
        let start = u32::try_from(found.pos).unwrap_or(u32::MAX);
        if locks::blocked(session, handle, start, u32::from(count)) {
            fail(cpu, session, 0x21);
            return;
        }
    }
    let Some(opened) = session.handles.get_mut(handle) else {
        fail(cpu, session, 6);
        return;
    };
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut transfer = TRANSFER.borrow();
    let mut seen = 0u16;
    for byte in &mut transfer[..usize::from(count)] {
        let Some(value) = ram.load(segment, offset) else {
            fail(cpu, session, 5);
            return;
        };
        *byte = value;
        offset = offset.wrapping_add(1);
        seen += 1;
    }
    if !super::disk::write_range(
        opened.dir,
        &opened.name,
        opened.pos,
        &transfer[..usize::from(seen)],
    ) {
        fail(cpu, session, 0x27);
        return;
    }
    opened.pos += usize::from(seen);
    opened.dirty = true;
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

fn write_console(cpu: &mut CpuState, ram: &GuestRam) {
    let count = cpu.wide(Slot::Count);
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut seen = 0u16;
    while seen < count {
        let mut text = [0u8; 64];
        let mut length = 0usize;
        while seen < count && length < text.len() {
            let Some(byte) = ram.load(segment, offset) else {
                break;
            };
            text[length] = byte;
            length += 1;
            seen += 1;
            offset = offset.wrapping_add(1);
        }
        if length == 0 {
            break;
        }
        crate::bios::mscon::con_write_bytes(&text[..length]);
    }
    cpu.set_wide(Slot::Accumulator, seen);
    cpu.set_carry(false);
}

// The loader copies the image into guest RAM before execution. Release this
// scratch space at that point so recursive EXEC can reuse it.
const EXEC_BYTES: usize = 640 * 1024;
static EXEC_IMAGE: KernelCell<[u8; EXEC_BYTES]> = KernelCell::new([0; EXEC_BYTES]);

fn launch(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) != 0 {
        fail(cpu, session, 1);
        return;
    }
    let text = guest_text(cpu, ram);
    let block_segment = cpu.selector(Segment::Extra);
    let block_offset = cpu.wide(Slot::Base);
    let Some((dir, name)) = search_spec(&text) else {
        fail(cpu, session, 2);
        return;
    };
    let given = read_word(ram, block_segment, block_offset).unwrap_or(0);
    let program = Program {
        directory: b"A:\\",
        name: &name,
        image: &[],
        preferred_psp: LAUNCH_SEGMENT,
        com_paragraphs: 0x1000,
        environment: if given == 0 {
            read_word(ram, session.current_psp, 0x2C).unwrap_or(0)
        } else {
            given
        },
        parent: session.current_psp,
        tail: CommandTail::from_exec(ram, block_segment, block_offset),
    };
    match run_child(dir, program, ram, session, Some(cpu)) {
        Ok(code) => {
            session.last_code = u16::from(code);
            cpu.set_wide(Slot::Accumulator, 0);
            cpu.set_carry(false);
        }
        Err(error) => fail(cpu, session, error),
    }
}

fn memory_status(cpu: &mut CpuState, session: &mut Session, result: Result<(), MemoryError>) {
    match result {
        Ok(()) => cpu.set_carry(false),
        Err(MemoryError::InvalidBlock) => fail(cpu, session, 9),
        Err(MemoryError::NoRoom(largest)) => {
            cpu.set_wide(Slot::Base, largest);
            fail(cpu, session, 8);
        }
    }
}

pub(super) fn spawn_at(
    dir: u32,
    args: &[u8],
    name: &[u8; 11],
    ram: &mut GuestRam,
    session: &mut Session,
) -> Option<u8> {
    let program = Program {
        directory: b"A:\\",
        name,
        image: &[],
        preferred_psp: SHELL_CHILD,
        com_paragraphs: 0x1000,
        environment: read_word(ram, session.current_psp, 0x2C).unwrap_or(0),
        parent: session.current_psp,
        tail: CommandTail::arguments(args),
    };
    match run_child(dir, program, ram, session, None) {
        Err(2) => None,
        result => Some(result.unwrap_or(0xff)),
    }
}

fn run_child(
    dir: u32,
    program: Program<'_>,
    ram: &mut GuestRam,
    session: &mut Session,
    caller: Option<&mut CpuState>,
) -> Result<u8, u16> {
    // Limit nested EXEC consistently for the interactive and batch entry points.
    if session.child_depth == 8 {
        return Err(8);
    }
    let version = session
        .drivers
        .program_version(ram, program.name)
        .unwrap_or(crate::dos::getset::VERSION.0);
    let (psp, mut cpu) = {
        let mut image = EXEC_IMAGE.borrow();
        let length = super::disk::read_file_at(dir, program.name, &mut image[..]).ok_or(2u16)?;
        let mut directory = [0; 68];
        directory[..3].copy_from_slice(b"A:\\");
        directory[0] += if super::disk::root(dir) != 0 {
            2
        } else {
            session.drive.min(1)
        };
        let mut end =
            3 + super::disk::in_dir(dir, || super::disk::current_path(&mut directory[3..67]));
        if end > 3 {
            directory[end] = b'\\';
            end += 1;
        }
        Program {
            directory: &directory[..end],
            image: &image[..length],
            ..program
        }
        .load(ram, &mut session.arena)?
    };
    let target = caller
        .as_ref()
        .map(|c| (c.selector(Segment::Code), c.instruction()))
        .unwrap_or((0, 0));
    termination::vector(session, ram, 0x22, target);
    termination::save_vectors(ram, psp);
    set_psp_version(ram, psp, version);
    let depth = session.child_depth as usize;
    session.parents[depth] = Some(Parent {
        registers: None,
        prefix: session.prefix,
        psp: session.current_psp,
        dta: (session.dta_segment, session.dta_offset),
        job: session.handles.inherit(),
    });
    session.handles.publish(ram, psp);
    session.prefix = psp;
    session.current_psp = psp;
    session.dta_segment = psp;
    session.dta_offset = 0x80;
    session.child_depth += 1;
    if session.scheduled {
        session.pending = Some(cpu);
        return Ok(0);
    }
    let end = drive(&mut cpu, ram, session, 8_000_000);
    if cfg!(feature = "batch-console") && !matches!(end, End::Stopped(_)) {
        match end {
            End::Fault(op) => {
                crate::puts("Unsupported opcode ");
                put_hex8(op);
            }
            End::Other(v) => {
                crate::puts("Unsupported interrupt ");
                put_hex8(v);
            }
            _ => crate::puts("Batch execution limit"),
        }
        crate::puts(" at ");
        crate::put_hex16(cpu.selector(Segment::Code));
        crate::putc(b':');
        crate::put_hex16(cpu.instruction());
        crate::puts("  ");
        for i in 0..6 {
            put_hex8(
                ram.load(
                    cpu.selector(Segment::Code),
                    cpu.instruction().wrapping_add(i),
                )
                .unwrap(),
            );
        }
        crate::puts("\n");
    }
    finish_child(session, ram, caller, matches!(end, End::Stopped(_)));
    match end {
        End::Stopped(code) => Ok(code),
        _ => Err(1),
    }
}

struct Parent {
    registers: Option<CpuState>,
    prefix: u16,
    psp: u16,
    dta: (u16, u16),
    job: handles::Job,
}

fn finish_child(
    session: &mut Session,
    ram: &mut GuestRam,
    caller: Option<&mut CpuState>,
    terminated: bool,
) -> Option<CpuState> {
    session.child_depth -= 1;
    let mut parent = session.parents[session.child_depth as usize]
        .take()
        .expect("paired child exit");
    if terminated
        && let Some(target) = termination::continuation(ram, session.prefix)
        && let Some(cpu) = parent.registers.as_mut().or(caller)
    {
        termination::resume(cpu, target);
    }
    if !session.handles.leave(parent.job) {
        crate::puts("Cannot commit child file.\n");
    }
    locks::discard_closed(session);
    session.arena.release_process(session.prefix);
    session.arena.publish(ram);
    session.prefix = parent.prefix;
    session.current_psp = parent.psp;
    (session.dta_segment, session.dta_offset) = parent.dta;
    parent.registers
}

fn set_psp_version(ram: &mut GuestRam, psp: u16, version: u16) {
    let [major, minor] = version.to_le_bytes();
    ram.store(psp, 0x40, major);
    ram.store(psp, 0x41, minor);
}

fn guest_text(cpu: &CpuState, ram: &GuestRam) -> [u8; 64] {
    let mut text = [0u8; 64];
    let mut offset = cpu.wide(Slot::Data);
    let segment = cpu.selector(Segment::Data);
    let mut index = 0usize;
    while index < text.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == 0 {
            break;
        }
        text[index] = byte;
        index += 1;
        offset = offset.wrapping_add(1);
    }
    text
}

fn system_values(cpu: &mut CpuState, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        0 => {
            cpu.set_low(Slot::Data, session.break_check);
            cpu.set_carry(false);
        }
        1 => {
            let requested = cpu.low(Slot::Data);
            if requested < 2 {
                session.break_check = requested;
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
        }
        2 => {
            let requested = cpu.low(Slot::Data);
            if requested < 2 {
                session.extended_break = requested;
                cpu.set_carry(false);
            } else {
                cpu.set_carry(true);
            }
        }
        5 => {
            cpu.set_low(Slot::Data, 1);
            cpu.set_carry(false);
        }
        _ => cpu.set_carry(true),
    }
}

fn parse_filename(cpu: &mut CpuState, ram: &mut GuestRam) {
    let options = cpu.low(Slot::Accumulator);
    let text_segment = cpu.selector(Segment::Data);
    let mut cursor = cpu.wide(Slot::Source);
    if options & 1 != 0 {
        while let Some(byte) = ram.load(text_segment, cursor) {
            if !leading_separator(byte) {
                break;
            }
            cursor = cursor.wrapping_add(1);
        }
    }
    let first = ram.load(text_segment, cursor).unwrap_or(0);
    let second = ram.load(text_segment, cursor.wrapping_add(1)).unwrap_or(0);
    if second == b':' {
        let letter = ascii_upper(first);
        if !letter.is_ascii_uppercase() {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_wide(Slot::Source, cursor);
            cpu.set_carry(false);
            return;
        }
        let _ = ram.store(
            cpu.selector(Segment::Extra),
            cpu.wide(Slot::Dest),
            letter - b'A' + 1,
        );
        cursor = cursor.wrapping_add(2);
    } else if options & 2 == 0 {
        let _ = ram.store(cpu.selector(Segment::Extra), cpu.wide(Slot::Dest), 0);
    }
    let mut wild = false;
    let name = read_name(ram, text_segment, &mut cursor, &mut wild);
    let dotted = ram.load(text_segment, cursor) == Some(b'.');
    if dotted {
        cursor = cursor.wrapping_add(1);
    }
    let extension = read_extension(ram, text_segment, &mut cursor, &mut wild, dotted);
    let record = cpu.selector(Segment::Extra);
    let base = cpu.wide(Slot::Dest);
    if name.written || options & 4 == 0 {
        write_field(ram, record, base.wrapping_add(1), &name.bytes, 8);
    }
    if extension.written || options & 8 == 0 {
        write_field(ram, record, base.wrapping_add(9), &extension.bytes, 3);
    }
    cpu.set_low(Slot::Accumulator, if wild { 1 } else { 0 });
    cpu.set_wide(Slot::Source, cursor);
    cpu.set_carry(false);
}

struct ParsedField {
    bytes: [u8; 8],
    written: bool,
}

fn read_name(ram: &GuestRam, segment: u16, cursor: &mut u16, wild: &mut bool) -> ParsedField {
    read_field(ram, segment, cursor, wild, 8, true)
}

fn read_extension(
    ram: &GuestRam,
    segment: u16,
    cursor: &mut u16,
    wild: &mut bool,
    dotted: bool,
) -> ParsedField {
    if !dotted {
        return ParsedField {
            bytes: [b' '; 8],
            written: false,
        };
    }
    read_field(ram, segment, cursor, wild, 3, false)
}

fn read_field(
    ram: &GuestRam,
    segment: u16,
    cursor: &mut u16,
    wild: &mut bool,
    width: usize,
    stop_on_dot: bool,
) -> ParsedField {
    let mut bytes = [b' '; 8];
    let mut index = 0usize;
    let mut written = false;
    while index < width {
        let Some(byte) = ram.load(segment, *cursor) else {
            break;
        };
        if byte == b'*' {
            *wild = true;
            written = true;
            while index < width {
                bytes[index] = b'?';
                index += 1;
            }
            *cursor = cursor.wrapping_add(1);
            while let Some(next) = ram.load(segment, *cursor) {
                if (stop_on_dot && next == b'.') || field_end(next) {
                    break;
                }
                *cursor = cursor.wrapping_add(1);
            }
            break;
        }
        if byte == b'?' {
            *wild = true;
            written = true;
            bytes[index] = b'?';
            index += 1;
            *cursor = cursor.wrapping_add(1);
            continue;
        }
        if (stop_on_dot && byte == b'.') || field_end(byte) {
            break;
        }
        written = true;
        bytes[index] = ascii_upper(byte);
        index += 1;
        *cursor = cursor.wrapping_add(1);
    }
    ParsedField { bytes, written }
}

fn write_field(ram: &mut GuestRam, segment: u16, offset: u16, bytes: &[u8; 8], width: usize) {
    let mut index = 0usize;
    while index < width {
        let _ = ram.store(segment, offset.wrapping_add(index as u16), bytes[index]);
        index += 1;
    }
}

fn field_end(byte: u8) -> bool {
    byte <= 0x20
        || matches!(
            byte,
            b'"' | b'+'
                | b','
                | b'/'
                | b':'
                | b';'
                | b'<'
                | b'='
                | b'>'
                | b'['
                | b'\\'
                | b']'
                | b'|'
        )
}

fn leading_separator(byte: u8) -> bool {
    field_end(byte) && byte != b'.' && byte != b':'
}

fn ascii_upper(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte.wrapping_sub(b'a').wrapping_add(b'A')
    } else {
        byte
    }
}

fn dos_name(text: &[u8; 64]) -> Option<[u8; 11]> {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut seen = false;
    for byte in text {
        if *byte == 0 {
            break;
        }
        seen = true;
        if *byte == b'.' && !extension {
            extension = true;
            continue;
        }
        let upper = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            *byte
        };
        if !extension && base < 8 {
            name[base] = upper;
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = upper;
            ext += 1;
        }
    }
    if seen { Some(name) } else { None }
}

fn print_marked(cpu: &CpuState, ram: &GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut text = [0u8; 64];
    let mut length = 0usize;
    for _ in 0..=u16::MAX {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == b'$' {
            break;
        }
        text[length] = byte;
        length += 1;
        offset = offset.wrapping_add(1);
        if length == text.len() {
            crate::bios::mscon::con_write_bytes(&text);
            length = 0;
        }
    }
    if length > 0 {
        crate::bios::mscon::con_write_bytes(&text[..length]);
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0F) as usize],
    ]);
}

fn named(cpu: &CpuState, ram: &GuestRam, act: fn(&[u8; 11]) -> bool) -> bool {
    let text = guest_text(cpu, ram);
    let Some((dir, name)) = search_spec(&text) else {
        return false;
    };
    super::disk::in_dir(dir, || act(&name)) && super::disk::flush(dir)
}

fn change_directory(cpu: &CpuState, ram: &GuestRam) -> bool {
    let text = guest_text(cpu, ram);
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    super::disk::change_dir(&text[..end])
}

fn rename_path(cpu: &mut CpuState, ram: &GuestRam) {
    let from = search_spec(&guest_text(cpu, ram));
    let to = search_spec(&bytes_at(
        ram,
        cpu.selector(Segment::Extra),
        cpu.wide(Slot::Dest),
    ));
    let ok = from.zip(to).is_some_and(|((source, old), (dest, new))| {
        source == dest
            && super::disk::in_dir(source, || super::disk::rename_file(&old, &new))
            && super::disk::flush(source)
    });
    if !ok {
        cpu.set_wide(Slot::Accumulator, 5);
    }
    cpu.set_carry(!ok);
}

fn file_time(cpu: &mut CpuState, session: &mut Session) {
    let sub = cpu.low(Slot::Accumulator);
    if sub > 1 {
        fail(cpu, session, 1);
        return;
    }
    let handle = cpu.wide(Slot::Base);
    if handle < 3 {
        fail(cpu, session, 6);
        return;
    }
    let looked = session
        .handles
        .get(handle)
        .map(|f| (f.dir, f.name, f.named, f.console));
    let Some((dir, name, named, console)) = looked else {
        fail(cpu, session, 6);
        return;
    };
    if !named || console {
        fail(cpu, session, 1);
        return;
    }
    if sub == 0 {
        let Some((time, date)) = super::disk::stamp_at(dir, &name) else {
            fail(cpu, session, 2);
            return;
        };
        cpu.set_wide(Slot::Count, time);
        cpu.set_wide(Slot::Data, date);
    } else {
        let time = cpu.wide(Slot::Count);
        let date = cpu.wide(Slot::Data);
        if !super::disk::set_stamp_at(dir, &name, time, date) {
            fail(cpu, session, 5);
            return;
        }
    }
    cpu.set_carry(false);
}

fn seek_file(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle < 3 {
        cpu.set_carry(true);
        return;
    }
    let Some(opened) = session.handles.get_mut(handle) else {
        cpu.set_carry(true);
        return;
    };
    let method = cpu.low(Slot::Accumulator);
    let offset = u32::from(cpu.wide(Slot::Count)) << 16 | u32::from(cpu.wide(Slot::Data));
    let next = match method {
        0 => offset,
        1 => (opened.pos as u32).wrapping_add(offset),
        2 => (super::disk::file_len_at(opened.dir, &opened.name).unwrap_or(0) as u32)
            .wrapping_add(offset),
        _ => {
            cpu.set_carry(true);
            return;
        }
    };
    opened.pos = next as usize;
    cpu.set_wide(Slot::Accumulator, next as u16);
    cpu.set_wide(Slot::Data, (next >> 16) as u16);
    cpu.set_carry(false);
}

fn read_attribute(cpu: &mut CpuState, ram: &GuestRam) {
    let Some((dir, name)) = search_spec(&guest_text(cpu, ram)) else {
        cpu.set_carry(true);
        return;
    };
    let action = cpu.low(Slot::Accumulator);
    if action == 0 {
        let Some(attr) = super::disk::in_dir(dir, || super::disk::attribute(&name)) else {
            cpu.set_wide(Slot::Accumulator, 2);
            cpu.set_carry(true);
            return;
        };
        cpu.set_wide(Slot::Count, u16::from(attr));
        cpu.set_carry(false);
        return;
    }
    if action == 1
        && super::disk::in_dir(dir, || {
            super::disk::set_attribute(&name, cpu.low(Slot::Count))
        })
        && super::disk::flush(dir)
    {
        cpu.set_carry(false);
        return;
    }
    cpu.set_wide(Slot::Accumulator, 5);
    cpu.set_carry(true);
}

fn truename(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let text = bytes_at(ram, cpu.selector(Segment::Data), cpu.wide(Slot::Source));
    let built = match canonical_path(&text, session.drive) {
        Ok(built) => built,
        Err(code) => {
            fail(cpu, session, code);
            return;
        }
    };
    let segment = cpu.selector(Segment::Extra);
    let mut offset = cpu.wide(Slot::Dest);
    let mut index = 0usize;
    while index < built.len {
        if !ram.store(segment, offset, built.bytes[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if ram.store(segment, offset, 0) {
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

struct Canon {
    bytes: [u8; 80],
    len: usize,
}

fn canonical_path(text: &[u8; 64], current: u8) -> Result<Canon, u16> {
    let mut cursor = 0usize;
    while cursor < text.len() && text[cursor] == b' ' {
        cursor += 1;
    }
    let mut end = cursor;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    if cursor >= end {
        return Err(3);
    }
    let mut drive = current;
    if cursor + 1 < end && text[cursor + 1] == b':' {
        let letter = ascii_upper(text[cursor]);
        if !letter.is_ascii_uppercase() {
            return Err(15);
        }
        drive = letter - b'A';
        if super::disk::drive_root(drive).is_none() {
            return Err(15);
        }
        cursor += 2;
    }
    if super::disk::drive_root(drive).is_none() {
        return Err(15);
    }
    let absolute = cursor < end && (text[cursor] == b'\\' || text[cursor] == b'/');
    let mut parts = [[0u8; 12]; 16];
    let mut lengths = [0u8; 16];
    let mut count = 0usize;
    if !absolute {
        let mut prefix = [0u8; 64];
        let dir = super::disk::drive_directory(drive).ok_or(15u16)?;
        let prefix_len = super::disk::in_dir(dir, || super::disk::current_path(&mut prefix));
        take_parts(&prefix[..prefix_len], &mut parts, &mut lengths, &mut count)?;
    }
    take_parts(&text[cursor..end], &mut parts, &mut lengths, &mut count)?;
    let mut bytes = [0u8; 80];
    bytes[0] = b'A' + drive;
    bytes[1] = b':';
    bytes[2] = b'\\';
    let mut used = 3usize;
    let mut index = 0usize;
    while index < count {
        if index > 0 {
            if used >= bytes.len() {
                return Err(3);
            }
            bytes[used] = b'\\';
            used += 1;
        }
        let length = usize::from(lengths[index]);
        let mut pos = 0usize;
        while pos < length {
            if used >= bytes.len() {
                return Err(3);
            }
            bytes[used] = parts[index][pos];
            used += 1;
            pos += 1;
        }
        index += 1;
    }
    Ok(Canon { bytes, len: used })
}

fn take_parts(
    text: &[u8],
    parts: &mut [[u8; 12]; 16],
    lengths: &mut [u8; 16],
    count: &mut usize,
) -> Result<(), u16> {
    let mut start = 0usize;
    while start <= text.len() {
        let mut stop = start;
        while stop < text.len() && text[stop] != b'\\' && text[stop] != b'/' {
            stop += 1;
        }
        push_part(parts, lengths, count, &text[start..stop])?;
        if stop >= text.len() {
            break;
        }
        start = stop + 1;
    }
    Ok(())
}

fn push_part(
    parts: &mut [[u8; 12]; 16],
    lengths: &mut [u8; 16],
    count: &mut usize,
    raw: &[u8],
) -> Result<(), u16> {
    if raw.is_empty() || raw == b"." {
        return Ok(());
    }
    if raw == b".." {
        if *count > 0 {
            *count -= 1;
        }
        return Ok(());
    }
    if *count >= parts.len() {
        return Err(3);
    }
    let mut name = [0u8; 12];
    let mut used = 0usize;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut dotted = false;
    for &byte in raw {
        let upper = ascii_upper(byte);
        if upper == b'.' {
            if dotted || used == 0 {
                return Err(3);
            }
            dotted = true;
            name[used] = b'.';
            used += 1;
            continue;
        }
        if !path_char(upper) {
            return Err(3);
        }
        if dotted {
            ext += 1;
            if ext > 3 {
                return Err(3);
            }
        } else {
            base += 1;
            if base > 8 {
                return Err(3);
            }
        }
        name[used] = upper;
        used += 1;
    }
    if used == 0 || (dotted && ext == 0) {
        return Err(3);
    }
    parts[*count] = name;
    lengths[*count] = used as u8;
    *count += 1;
    Ok(())
}

fn path_char(byte: u8) -> bool {
    byte.is_ascii_uppercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'-'
                | b'@'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'}'
                | b'~'
        )
}

fn current_directory(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let drive = cpu.low(Slot::Data);
    let Some(dir) = volume_for(drive) else {
        fail(cpu, session, 15);
        return;
    };
    let mut path = [0u8; 64];
    let length = super::disk::in_dir(dir, || super::disk::current_path(&mut path));
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Source);
    let mut index = 0usize;
    while index < length {
        if !ram.store(segment, offset, path[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    if ram.store(segment, offset, 0) {
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn find_first(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let text = guest_text(cpu, ram);
    if let Some(letter) = drive_letter(&text)
        && letter != b'A'
        && !(letter == b'C' && super::disk::has_c())
    {
        fail(cpu, session, 15);
        return;
    }
    let Some((dir, template)) = search_spec(&text) else {
        fail(cpu, session, 3);
        return;
    };
    let attr = cpu.low(Slot::Count);
    let Some(found) = super::disk::find_match(dir, &template, attr, 0) else {
        fail(cpu, session, 2);
        return;
    };
    if write_match(ram, session, dir, &template, attr, &found) {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn find_next(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let (segment, base) = dta_target(session);
    let mut template = [b' '; 11];
    let mut index = 0usize;
    while index < template.len() {
        let Some(byte) = ram.load(segment, base.wrapping_add(1 + index as u16)) else {
            fail(cpu, session, 18);
            return;
        };
        template[index] = byte;
        index += 1;
    }
    let Some(attr) = ram.load(segment, base.wrapping_add(12)) else {
        fail(cpu, session, 18);
        return;
    };
    let Some(start) = read_word(ram, segment, base.wrapping_add(13)) else {
        fail(cpu, session, 18);
        return;
    };
    let mut directory = [0; 4];
    if !load_bytes(ram, segment, base.wrapping_add(15), &mut directory) {
        fail(cpu, session, 18);
        return;
    };
    let dir = u32::from_le_bytes(directory);
    let Some(found) = super::disk::find_match(dir, &template, attr, start) else {
        fail(cpu, session, 18);
        return;
    };
    if write_match(ram, session, dir, &template, attr, &found) {
        cpu.set_wide(Slot::Accumulator, 0);
        cpu.set_carry(false);
    } else {
        fail(cpu, session, 5);
    }
}

fn drive_letter(text: &[u8; 64]) -> Option<u8> {
    if text.len() >= 2 && text[1] == b':' {
        Some(ascii_upper(text[0]))
    } else {
        None
    }
}

fn search_spec(text: &[u8; 64]) -> Option<(u32, [u8; 11])> {
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    if end == 0 {
        return None;
    }
    let (mut dir, text) = super::disk::path_start(&text[..end])?;
    let cursor = 0;
    let end = text.len();
    let mut last = cursor;
    let mut index = cursor;
    while index < end {
        if text[index] == b'\\' || text[index] == b'/' {
            dir = directory_at(dir, &text[last..index])?;
            last = index + 1;
        }
        index += 1;
    }
    if last >= end {
        return None;
    }
    Some((dir, component_template(&text[last..end])))
}

fn directory_at(parent: u32, text: &[u8]) -> Option<u32> {
    if text == b"." || text.is_empty() {
        return Some(parent);
    }
    if text == b".." {
        return Some(super::disk::parent_directory(parent));
    }
    let mut padded = [0u8; 64];
    let count = text.len().min(padded.len() - 1);
    let mut index = 0usize;
    while index < count {
        padded[index] = text[index];
        index += 1;
    }
    let name = dos_name(&padded)?;
    super::disk::directory_cluster(parent, &name)
}

fn component_template(text: &[u8]) -> [u8; 11] {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut fill = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let upper = ascii_upper(text[index]);
        if upper == b'.' && !extension {
            if fill {
                while base < 8 {
                    name[base] = b'?';
                    base += 1;
                }
            }
            extension = true;
            fill = false;
            index += 1;
            continue;
        }
        if upper == b'*' {
            fill = true;
            index += 1;
            continue;
        }
        if !extension && base < 8 {
            name[base] = if fill { b'?' } else { upper };
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = if fill { b'?' } else { upper };
            ext += 1;
        }
        index += 1;
    }
    if fill {
        if extension {
            while ext < 3 {
                name[8 + ext] = b'?';
                ext += 1;
            }
        } else {
            while base < 8 {
                name[base] = b'?';
                base += 1;
            }
        }
    }
    name
}

fn write_match(
    ram: &mut GuestRam,
    session: &Session,
    dir: u32,
    template: &[u8; 11],
    search_attr: u8,
    found: &super::disk::DirMatch,
) -> bool {
    let (segment, base) = dta_target(session);
    if !ram.store(segment, base, 1) {
        return false;
    }
    let mut index = 0usize;
    while index < template.len() {
        if !ram.store(
            segment,
            base.wrapping_add(1 + index as u16),
            template[index],
        ) {
            return false;
        }
        index += 1;
    }
    if !ram.store(segment, base.wrapping_add(12), search_attr) {
        return false;
    }
    let next = found.index.wrapping_add(1).to_le_bytes();
    if !ram.store(segment, base.wrapping_add(13), next[0])
        || !ram.store(segment, base.wrapping_add(14), next[1])
    {
        return false;
    }
    let place = dir.to_le_bytes();
    if !store_bytes(ram, segment, base.wrapping_add(15), &place) {
        return false;
    }
    if !ram.store(segment, base.wrapping_add(0x15), found.attr) {
        return false;
    }
    let stamp_time = found.time.to_le_bytes();
    let stored_date = if found.date == 0 { 0x0021 } else { found.date };
    let stamp_date = stored_date.to_le_bytes();
    if !ram.store(segment, base.wrapping_add(0x16), stamp_time[0])
        || !ram.store(segment, base.wrapping_add(0x17), stamp_time[1])
        || !ram.store(segment, base.wrapping_add(0x18), stamp_date[0])
        || !ram.store(segment, base.wrapping_add(0x19), stamp_date[1])
    {
        return false;
    }
    let size = found.size.to_le_bytes();
    let mut part = 0usize;
    while part < size.len() {
        if !ram.store(
            segment,
            base.wrapping_add(0x1A).wrapping_add(part as u16),
            size[part],
        ) {
            return false;
        }
        part += 1;
    }
    let mut shown = [0u8; 13];
    let mut length = 0usize;
    let mut cursor = 0usize;
    while cursor < 8 && found.name[cursor] != b' ' {
        shown[length] = found.name[cursor];
        length += 1;
        cursor += 1;
    }
    if found.name[8] != b' ' {
        shown[length] = b'.';
        length += 1;
        cursor = 8;
        while cursor < 11 && found.name[cursor] != b' ' {
            shown[length] = found.name[cursor];
            length += 1;
            cursor += 1;
        }
    }
    if length + 1 < shown.len() {
        shown[length + 1] = b'\r';
    }
    if length + 2 < shown.len() {
        shown[length + 2] = b'\n';
    }
    if length + 3 < shown.len() {
        shown[length + 3] = b'$';
    }
    let mut slot = 0usize;
    while slot < shown.len() {
        if !ram.store(
            segment,
            base.wrapping_add(0x1E).wrapping_add(slot as u16),
            shown[slot],
        ) {
            return false;
        }
        slot += 1;
    }
    true
}

fn dta_target(session: &Session) -> (u16, u16) {
    (session.dta_segment, session.dta_offset)
}

fn list_of_lists(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session) {
    let Some((_per_cluster, per_sector, _free, _total)) = super::disk::allocation() else {
        cpu.set_carry(true);
        return;
    };
    let (segment, offset) = session.drivers.list_address();
    store_bytes(
        ram,
        segment,
        offset - 2,
        &super::arena::FIRST_MCB.to_le_bytes(),
    );
    let sector = per_sector.to_le_bytes();
    let mut raw = [0u8; 0x34];
    session.handles.publish_sft(ram);
    raw[6..8].copy_from_slice(&handles::SFT_SEGMENT.to_le_bytes());
    raw[0x10] = sector[0];
    raw[0x11] = sector[1];
    raw[0x20] = 2;
    raw[0x21] = session.drivers.count();
    let header: [u8; 18] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x04, 0x80, 0, 0, 0, 0, b'N', b'U', b'L', b' ', b' ', b' ', b' ',
        b' ',
    ];
    raw[0x22..0x34].copy_from_slice(&header);
    raw[0x22..0x26].copy_from_slice(&session.drivers.head());
    let mut index = 0u16;
    while usize::from(index) < raw.len() {
        if !ram.store(segment, offset.wrapping_add(index), raw[usize::from(index)]) {
            cpu.set_carry(true);
            return;
        }
        index = index.wrapping_add(1);
    }
    cpu.set_selector(Segment::Extra, segment);
    cpu.set_wide(Slot::Base, offset);
    cpu.set_carry(false);
}

fn drive_parameters(cpu: &mut CpuState, ram: &mut GuestRam) {
    let drive = cpu.low(Slot::Data);
    let Some(block) =
        volume_for(drive).and_then(|d| super::disk::in_dir(d, super::disk::drive_block))
    else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let mut raw = [0u8; 0x20];
    let sector = block.bytes_per_sector.to_le_bytes();
    raw[2] = sector[0];
    raw[3] = sector[1];
    raw[4] = block.sectors_per_cluster.wrapping_sub(1);
    raw[5] = block.shift;
    let reserved = block.reserved.to_le_bytes();
    raw[6] = reserved[0];
    raw[7] = reserved[1];
    raw[8] = block.fats;
    let root = block.root_entries.to_le_bytes();
    raw[9] = root[0];
    raw[10] = root[1];
    let data = block.data_sector.to_le_bytes();
    raw[0x0B] = data[0];
    raw[0x0C] = data[1];
    let highest = block.max_cluster.to_le_bytes();
    raw[0x0D] = highest[0];
    raw[0x0E] = highest[1];
    let fat = block.sectors_per_fat.to_le_bytes();
    raw[0x0F] = fat[0];
    raw[0x10] = fat[1];
    raw[0x11] = block.media;
    const SEGMENT: u16 = 0x60;
    let mut index = 0u16;
    while usize::from(index) < raw.len() {
        if !ram.store(SEGMENT, index, raw[usize::from(index)]) {
            cpu.set_low(Slot::Accumulator, 0xFF);
            cpu.set_carry(true);
            return;
        }
        index = index.wrapping_add(1);
    }
    cpu.set_selector(Segment::Data, SEGMENT);
    cpu.set_wide(Slot::Base, 0);
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn indos_flag(cpu: &mut CpuState, ram: &mut GuestRam) {
    const SEGMENT: u16 = 0x50;
    const OFFSET: u16 = 4;
    if !ram.store(SEGMENT, OFFSET, 0) || !ram.store(SEGMENT, OFFSET.wrapping_add(1), 0) {
        cpu.set_carry(true);
        return;
    }
    cpu.set_selector(Segment::Extra, SEGMENT);
    cpu.set_wide(Slot::Base, OFFSET);
    cpu.set_carry(false);
}

fn drive_info(cpu: &mut CpuState, ram: &mut GuestRam) {
    let drive = cpu.low(Slot::Data);
    let Some(dir) = volume_for(drive) else {
        cpu.set_low(Slot::Accumulator, 0xff);
        cpu.set_carry(true);
        return;
    };
    let Some((per_cluster, per_sector, _free, total)) = super::disk::allocation_at(dir) else {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    };
    let media = super::disk::in_dir(dir, super::disk::media_id);
    if !ram.store(0x50, 0, media) {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(true);
        return;
    }
    cpu.set_low(Slot::Accumulator, per_cluster as u8);
    cpu.set_wide(Slot::Count, per_sector);
    cpu.set_wide(Slot::Data, total);
    cpu.set_selector(Segment::Data, 0x50);
    cpu.set_wide(Slot::Base, 0);
    cpu.set_carry(false);
}

fn volume_serial(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let drive = cpu.low(Slot::Base);
    let Some(dir) = volume_for(drive) else {
        fail(cpu, session, 15);
        return;
    };
    let segment = cpu.selector(Segment::Data);
    let offset = cpu.wide(Slot::Data);
    if read_word(ram, segment, offset) != Some(0) {
        fail(cpu, session, 0x57);
        return;
    }
    match cpu.low(Slot::Accumulator) {
        0 => {
            let Some((serial, label, kind)) = super::disk::in_dir(dir, super::disk::volume_id)
            else {
                fail(cpu, session, 15);
                return;
            };
            if !store_bytes(ram, segment, offset.wrapping_add(2), &serial)
                || !store_bytes(ram, segment, offset.wrapping_add(6), &label)
                || !store_bytes(ram, segment, offset.wrapping_add(17), &kind)
            {
                fail(cpu, session, 5);
                return;
            }
            cpu.set_carry(false);
        }
        1 => {
            let mut serial = [0u8; 4];
            let mut label = [b' '; 11];
            if !load_bytes(ram, segment, offset.wrapping_add(2), &mut serial)
                || !load_bytes(ram, segment, offset.wrapping_add(6), &mut label)
            {
                fail(cpu, session, 5);
                return;
            }
            if super::disk::in_dir(dir, || super::disk::set_volume_id(&serial, &label))
                && super::disk::flush(dir)
            {
                cpu.set_carry(false);
            } else {
                fail(cpu, session, 15);
            }
        }
        _ => fail(cpu, session, 1),
    }
}

fn store_bytes(ram: &mut GuestRam, segment: u16, mut offset: u16, bytes: &[u8]) -> bool {
    for byte in bytes {
        if !ram.store(segment, offset, *byte) {
            return false;
        }
        offset = offset.wrapping_add(1);
    }
    true
}

fn load_bytes(ram: &GuestRam, segment: u16, mut offset: u16, bytes: &mut [u8]) -> bool {
    for byte in bytes {
        let Some(loaded) = ram.load(segment, offset) else {
            return false;
        };
        *byte = loaded;
        offset = offset.wrapping_add(1);
    }
    true
}

fn switch_character(cpu: &mut CpuState, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        0 => {
            cpu.set_low(Slot::Data, session.switch_char);
            cpu.set_carry(false);
        }
        1 => {
            let requested = cpu.low(Slot::Data);
            if !requested.is_ascii_graphic() {
                fail(cpu, session, 1);
                return;
            }
            session.switch_char = requested;
            cpu.set_carry(false);
        }
        _ => fail(cpu, session, 1),
    }
}

fn uppercase_info(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    if cpu.low(Slot::Accumulator) == 0x23 {
        yes_or_no(cpu);
        return;
    }
    if cpu.low(Slot::Accumulator) == 0x20 {
        capitalize_one(cpu);
        return;
    }
    if cpu.low(Slot::Accumulator) == 0x21 {
        capitalize_count(cpu, ram);
        return;
    }
    if cpu.low(Slot::Accumulator) == 0x22 {
        capitalize_text(cpu, ram);
        return;
    }
    if cpu.low(Slot::Accumulator) == 1 {
        extended_country(cpu, ram, session);
        return;
    }
    if cpu.low(Slot::Accumulator) == 5 {
        filename_chars(cpu, ram, session);
        return;
    }
    if cpu.low(Slot::Accumulator) == 6 {
        collating_info(cpu, ram, session);
        return;
    }
    if cpu.low(Slot::Accumulator) == 7 {
        dbcs_info(cpu, ram, session);
        return;
    }
    if cpu.low(Slot::Accumulator) != 2 {
        fail(cpu, session, 1);
        return;
    }
    let page = cpu.wide(Slot::Base);
    let country = cpu.wide(Slot::Data);
    if page != 0xFFFF && page != session.code_page {
        fail(cpu, session, 2);
        return;
    }
    if country != 0xFFFF && country != 1 {
        fail(cpu, session, 2);
        return;
    }
    if cpu.wide(Slot::Count) < 5 {
        fail(cpu, session, 1);
        return;
    }
    if !plant_uppercase(ram) {
        fail(cpu, session, 5);
        return;
    }
    let segment = cpu.selector(Segment::Extra);
    let offset = cpu.wide(Slot::Dest);
    let pointer = UPPERCASE_SEGMENT.to_le_bytes();
    if !ram.store(segment, offset, 2)
        || !ram.store(segment, offset.wrapping_add(1), 0)
        || !ram.store(segment, offset.wrapping_add(2), 0)
        || !ram.store(segment, offset.wrapping_add(3), pointer[0])
        || !ram.store(segment, offset.wrapping_add(4), pointer[1])
    {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_carry(false);
}

fn capitalize_text(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let origin = cpu.wide(Slot::Data);
    let mut offset = origin;
    loop {
        let Some(byte) = ram.load(segment, offset) else {
            cpu.set_carry(true);
            return;
        };
        if byte == 0 {
            cpu.set_carry(false);
            return;
        }
        let upper = upper_letter(byte);
        if !ram.store(segment, offset, upper) {
            cpu.set_carry(true);
            return;
        }
        let next = offset.wrapping_add(1);
        if next == origin {
            cpu.set_carry(true);
            return;
        }
        offset = next;
    }
}

fn yes_or_no(cpu: &mut CpuState) {
    let class = match cpu.low(Slot::Data) {
        b'Y' | b'y' => 0,
        b'N' | b'n' => 1,
        _ => 2,
    };
    cpu.set_wide(Slot::Accumulator, class);
    cpu.set_carry(false);
}

fn capitalize_one(cpu: &mut CpuState) {
    let byte = cpu.low(Slot::Data);
    cpu.set_low(Slot::Data, upper_letter(byte));
    cpu.set_carry(false);
}

fn capitalize_count(cpu: &mut CpuState, ram: &mut GuestRam) {
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    let mut left = cpu.wide(Slot::Count);
    while left > 0 {
        let Some(byte) = ram.load(segment, offset) else {
            cpu.set_carry(true);
            return;
        };
        if !ram.store(segment, offset, upper_letter(byte)) {
            cpu.set_carry(true);
            return;
        }
        offset = offset.wrapping_add(1);
        left -= 1;
    }
    cpu.set_carry(false);
}

fn upper_letter(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte.wrapping_sub(b'a').wrapping_add(b'A')
    } else {
        byte
    }
}

fn extended_country(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let page = cpu.wide(Slot::Base);
    let country = cpu.wide(Slot::Data);
    if page != 0xFFFF && page != session.code_page {
        fail(cpu, session, 2);
        return;
    }
    if country != 0xFFFF && country != 1 {
        fail(cpu, session, 2);
        return;
    }
    let limit = usize::from(cpu.wide(Slot::Count));
    if limit < 7 {
        fail(cpu, session, 1);
        return;
    }
    let mut raw = [0u8; 41];
    raw[0] = 1;
    raw[1] = 0x26;
    raw[3] = 1;
    raw[5] = 0xB5;
    raw[6] = 0x01;
    raw[9] = b'$';
    raw[0x0E] = b',';
    raw[0x10] = b'.';
    raw[0x12] = b'-';
    raw[0x14] = b':';
    raw[0x17] = 2;
    raw[0x1D] = b',';
    let segment = cpu.selector(Segment::Extra);
    let mut offset = cpu.wide(Slot::Dest);
    let mut index = 0usize;
    let count = limit.min(raw.len());
    while index < count {
        if !ram.store(segment, offset, raw[index]) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
        index += 1;
    }
    cpu.set_carry(false);
}

fn filename_chars(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let page = cpu.wide(Slot::Base);
    let country = cpu.wide(Slot::Data);
    if page != 0xFFFF && page != session.code_page {
        fail(cpu, session, 2);
        return;
    }
    if country != 0xFFFF && country != 1 {
        fail(cpu, session, 2);
        return;
    }
    if cpu.wide(Slot::Count) < 5 {
        fail(cpu, session, 1);
        return;
    }
    if !plant_filename_chars(ram) {
        fail(cpu, session, 5);
        return;
    }
    let segment = cpu.selector(Segment::Extra);
    let offset = cpu.wide(Slot::Dest);
    let pointer = FILENAME_SEGMENT.to_le_bytes();
    if !ram.store(segment, offset, 5)
        || !ram.store(segment, offset.wrapping_add(1), 0)
        || !ram.store(segment, offset.wrapping_add(2), 0)
        || !ram.store(segment, offset.wrapping_add(3), pointer[0])
        || !ram.store(segment, offset.wrapping_add(4), pointer[1])
    {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_carry(false);
}

fn plant_filename_chars(ram: &mut GuestRam) -> bool {
    let length = 0x16u16.to_le_bytes();
    if !ram.store(FILENAME_SEGMENT, 0, length[0])
        || !ram.store(FILENAME_SEGMENT, 1, length[1])
        || !ram.store(FILENAME_SEGMENT, 2, 0)
        || !ram.store(FILENAME_SEGMENT, 3, 0xFF)
    {
        return false;
    }
    let mut index = 4u16;
    while index < 24 {
        if !ram.store(FILENAME_SEGMENT, index, 0) {
            return false;
        }
        index = index.wrapping_add(1);
    }
    true
}

const FILENAME_SEGMENT: u16 = 0x90;
const UPPERCASE_SEGMENT: u16 = 0x64;
const DBCS_SEGMENT: u16 = 0x6D;

fn collating_info(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let page = cpu.wide(Slot::Base);
    let country = cpu.wide(Slot::Data);
    if page != 0xFFFF && page != session.code_page {
        fail(cpu, session, 2);
        return;
    }
    if country != 0xFFFF && country != 1 {
        fail(cpu, session, 2);
        return;
    }
    if cpu.wide(Slot::Count) < 5 {
        fail(cpu, session, 1);
        return;
    }
    if !plant_collating(ram) {
        fail(cpu, session, 5);
        return;
    }
    let segment = cpu.selector(Segment::Extra);
    let offset = cpu.wide(Slot::Dest);
    let pointer = COLLATING_SEGMENT.to_le_bytes();
    if !ram.store(segment, offset, 6)
        || !ram.store(segment, offset.wrapping_add(1), 0)
        || !ram.store(segment, offset.wrapping_add(2), 0)
        || !ram.store(segment, offset.wrapping_add(3), pointer[0])
        || !ram.store(segment, offset.wrapping_add(4), pointer[1])
    {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_carry(false);
}

fn plant_collating(ram: &mut GuestRam) -> bool {
    let length = 0x100u16.to_le_bytes();
    if !ram.store(COLLATING_SEGMENT, 0, length[0]) || !ram.store(COLLATING_SEGMENT, 1, length[1]) {
        return false;
    }
    let mut index = 0u16;
    while index < 0x100 {
        let Ok(byte) = u8::try_from(index) else {
            return false;
        };
        if !ram.store(COLLATING_SEGMENT, index.wrapping_add(2), byte) {
            return false;
        }
        index = index.wrapping_add(1);
    }
    true
}

const COLLATING_SEGMENT: u16 = 0x78;

fn dbcs_info(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let page = cpu.wide(Slot::Base);
    let country = cpu.wide(Slot::Data);
    if page != 0xFFFF && page != session.code_page {
        fail(cpu, session, 2);
        return;
    }
    if country != 0xFFFF && country != 1 {
        fail(cpu, session, 2);
        return;
    }
    if cpu.wide(Slot::Count) < 5 {
        fail(cpu, session, 1);
        return;
    }
    const TABLE: u16 = DBCS_SEGMENT;
    if !ram.store(TABLE, 0, 0) || !ram.store(TABLE, 1, 0) {
        fail(cpu, session, 5);
        return;
    }
    let segment = cpu.selector(Segment::Extra);
    let offset = cpu.wide(Slot::Dest);
    let pointer = TABLE.to_le_bytes();
    if !ram.store(segment, offset, 7)
        || !ram.store(segment, offset.wrapping_add(1), 0)
        || !ram.store(segment, offset.wrapping_add(2), 0)
        || !ram.store(segment, offset.wrapping_add(3), pointer[0])
        || !ram.store(segment, offset.wrapping_add(4), pointer[1])
    {
        fail(cpu, session, 5);
        return;
    }
    cpu.set_carry(false);
}

fn plant_uppercase(ram: &mut GuestRam) -> bool {
    const SEGMENT: u16 = UPPERCASE_SEGMENT;
    if !ram.store(SEGMENT, 0, 0x80) || !ram.store(SEGMENT, 1, 0) {
        return false;
    }
    let mut index = 0u16;
    while index < 0x80 {
        let byte = 0x80u8.wrapping_add(index as u8);
        if !ram.store(SEGMENT, index.wrapping_add(2), byte) {
            return false;
        }
        index = index.wrapping_add(1);
    }
    true
}

fn code_page(cpu: &mut CpuState, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        1 => {
            cpu.set_wide(Slot::Base, session.code_page);
            cpu.set_wide(Slot::Data, 437);
            cpu.set_carry(false);
        }
        2 => {
            let requested = cpu.wide(Slot::Base);
            if requested != 437 {
                fail(cpu, session, 2);
                return;
            }
            session.code_page = requested;
            cpu.set_carry(false);
        }
        _ => fail(cpu, session, 1),
    }
}

fn country_info(cpu: &mut CpuState, ram: &mut GuestRam, session: &mut Session) {
    let code = match cpu.low(Slot::Accumulator) {
        0 => 1,
        0xFF => cpu.wide(Slot::Base),
        other => u16::from(other),
    };
    if code != 1 {
        fail(cpu, session, 2);
        return;
    }
    let mut info = [0u8; 34];
    info[2] = b'$';
    info[7] = b',';
    info[9] = b'.';
    info[0x0B] = b'-';
    info[0x0D] = b':';
    info[0x10] = 2;
    info[0x16] = b',';
    let segment = cpu.selector(Segment::Data);
    let mut offset = cpu.wide(Slot::Data);
    for byte in info {
        if !ram.store(segment, offset, byte) {
            fail(cpu, session, 5);
            return;
        }
        offset = offset.wrapping_add(1);
    }
    cpu.set_wide(Slot::Base, 1);
    cpu.set_carry(false);
}

fn volume_for(dos_drive: u8) -> Option<u32> {
    if dos_drive == 0 {
        Some(super::disk::working_directory())
    } else {
        super::disk::drive_directory(dos_drive - 1)
    }
}

fn report_space(cpu: &mut CpuState) {
    let Some((per_cluster, per_sector, free_units, total_units)) =
        volume_for(cpu.low(Slot::Data)).and_then(super::disk::allocation_at)
    else {
        cpu.set_wide(Slot::Accumulator, 0xFFFF);
        cpu.set_carry(true);
        return;
    };
    cpu.set_wide(Slot::Accumulator, per_cluster);
    cpu.set_wide(Slot::Base, free_units);
    cpu.set_wide(Slot::Count, per_sector);
    cpu.set_wide(Slot::Data, total_units);
    cpu.set_carry(false);
}

fn bytes_at(ram: &GuestRam, segment: u16, mut offset: u16) -> [u8; 64] {
    let mut text = [0u8; 64];
    let mut index = 0usize;
    while index < text.len() {
        let Some(byte) = ram.load(segment, offset) else {
            break;
        };
        if byte == 0 {
            break;
        }
        text[index] = byte;
        index += 1;
        offset = offset.wrapping_add(1);
    }
    text
}

fn device_control(cpu: &mut CpuState, session: &mut Session) {
    match cpu.low(Slot::Accumulator) {
        0 => {
            cpu.set_wide(Slot::Data, file_device_word(session, cpu.wide(Slot::Base)));
            cpu.set_carry(false);
        }
        6 => input_status(cpu, session),
        7 => output_status(cpu, session),
        8 => removable_drive(cpu, session),
        9 => local_drive(cpu, session),
        0x0A => local_handle(cpu, session),
        0x0E => logical_map(cpu, session),
        _ => cpu.set_carry(true),
    }
}

fn logical_map(cpu: &mut CpuState, session: &mut Session) {
    let drive = cpu.low(Slot::Base);
    if drive > 1 {
        fail(cpu, session, 15);
        return;
    }
    cpu.set_low(Slot::Accumulator, 0);
    cpu.set_carry(false);
}

fn local_handle(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle >= 3 {
        let Some(_) = session.handles.get(handle) else {
            fail(cpu, session, 6);
            return;
        };
    }
    cpu.set_wide(Slot::Data, 0);
    cpu.set_carry(false);
}

fn local_drive(cpu: &mut CpuState, session: &mut Session) {
    let drive = cpu.low(Slot::Base);
    if volume_for(drive).is_none() {
        fail(cpu, session, 15);
        return;
    }
    cpu.set_wide(Slot::Data, 0);
    cpu.set_carry(false);
}

fn removable_drive(cpu: &mut CpuState, session: &mut Session) {
    let drive = cpu.low(Slot::Base);
    if volume_for(drive).is_none() {
        fail(cpu, session, 15);
        return;
    }
    cpu.set_wide(
        Slot::Accumulator,
        u16::from(volume_for(drive).is_some_and(|d| super::disk::root(d) != 0)),
    );
    cpu.set_carry(false);
}

fn output_status(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle == 0 {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
        return;
    }
    if handle < 3 {
        cpu.set_low(Slot::Accumulator, 0xFF);
        cpu.set_carry(false);
        return;
    }
    let Some(_) = session.handles.get(handle) else {
        fail(cpu, session, 6);
        return;
    };
    cpu.set_low(Slot::Accumulator, 0xFF);
    cpu.set_carry(false);
}

fn input_status(cpu: &mut CpuState, session: &mut Session) {
    let handle = cpu.wide(Slot::Base);
    if handle < 3 {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
        return;
    }
    let Some(opened) = session.handles.get(handle) else {
        fail(cpu, session, 6);
        return;
    };
    if opened.console {
        cpu.set_low(Slot::Accumulator, 0);
        cpu.set_carry(false);
        return;
    }
    let ready = if opened.pos < super::disk::file_len_at(opened.dir, &opened.name).unwrap_or(0) {
        0xFF
    } else {
        0
    };
    cpu.set_low(Slot::Accumulator, ready);
    cpu.set_carry(false);
}

fn file_device_word(session: &Session, handle: u16) -> u16 {
    session.handles.get(handle).map_or(0x0040, |f| {
        if f.console {
            0x80d3
        } else {
            written_file_word(f.dirty)
        }
    })
}

fn written_file_word(written: bool) -> u16 {
    if written { 0 } else { 0x0040 }
}

fn enter_vector(cpu: &mut CpuState, ram: &mut GuestRam, session: &Session, vector: u8) -> bool {
    let id = usize::from(vector);
    if !session.vector_live[id] {
        return false;
    }
    let flags = cpu.flags();
    let caller = cpu.selector(Segment::Code);
    let next = cpu.instruction();
    if !guest_push(cpu, ram, flags) || !guest_push(cpu, ram, caller) || !guest_push(cpu, ram, next)
    {
        return false;
    }
    cpu.set_selector(Segment::Code, session.vector_segment[id]);
    cpu.set_instruction(session.vector_offset[id]);
    true
}

fn guest_push(cpu: &mut CpuState, ram: &mut GuestRam, value: u16) -> bool {
    let pointer = cpu.wide(Slot::Stack).wrapping_sub(2);
    cpu.set_wide(Slot::Stack, pointer);
    let bytes = value.to_le_bytes();
    let segment = cpu.selector(Segment::Stack);
    ram.store(segment, pointer, bytes[0]) && ram.store(segment, pointer.wrapping_add(1), bytes[1])
}
