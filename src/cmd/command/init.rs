//! Startup from `CMD/COMMAND/INIT.ASM`.
//!
//! A bad DOS version hangs when COMMAND is its own parent and exits otherwise.
//! Append is cleared only after both the install check and the version check.
//! Switches are recognized in order: `/F`, `/P`, `/D`, `/C`, `/E`, `/MSG`.
//! A second copy of a switch is too many arguments. `/C` stops the scan and
//! clears the permanent flag. `/MSG` without `/P` is a missing parameter.

const CONTINUE: u8 = 0;
const HANG: u8 = 1;
const EXIT: u8 = 2;
const ENV_MINIMUM: u16 = 160;
const TOO_MANY: u8 = 1;
const BAD_SWITCH: u8 = 3;
const MISSING: u8 = 2;
const RESIDENT_MESSAGES: u8 = 1;
const DISK_MESSAGES: u8 = 3;
const BAD_DIRECTORY: u16 = 465;
const ACCESS_DIRECTORY: u16 = 466;
const ACCESS_ERROR: u16 = 65;

#[derive(Clone, Copy)]
enum Item {
    Fail,
    Permanent,
    NoDate,
    Single,
    Environment(u16),
    Messages,
    Invalid { text: &'static [u8], switch_char: u8 },
}

struct Startup {
    fail: u8,
    permanent: u8,
    prompt: u8,
    single: u8,
    error: u8,
    messages: u8,
    paragraphs: u16,
    date_seen: u8,
    size_seen: u8,
}

fn version_fate(load_ok: bool, own_parent: bool) -> u8 {
    if load_ok {
        CONTINUE
    } else if own_parent {
        HANG
    } else {
        EXIT
    }
}

fn append_cleared(installed: u8, version: u16) -> bool {
    if installed == 0 {
        return false;
    }
    version == 0xFFFF
}

fn separator(switch_char: u8, current: u8) -> u8 {
    if switch_char == b'/' { b'\\' } else { current }
}

fn command_unit(zero_based: u8) -> u8 {
    zero_based.wrapping_add(1)
}

fn drive_letter(unit: u8) -> u8 {
    unit.wrapping_add(0x40)
}

fn low_buffer(resident: u16) -> u16 {
    let high = resident & 0xF000;
    let aligned = high.wrapping_add(0x1000);
    if aligned < high { resident } else { aligned }
}

fn env_paragraphs(bytes: u16) -> u16 {
    bytes.wrapping_add(15) >> 4
}

fn path_char(switch_char: u8, byte: u8) -> bool {
    if switch_char != b'/' && byte == b'/' {
        return true;
    }
    byte == b'\\'
}

fn comspec_count(ends_with_separator: bool) -> u16 {
    if ends_with_separator { 13 } else { 14 }
}

fn comspec_message(error: u16) -> u16 {
    if error == ACCESS_ERROR { ACCESS_DIRECTORY } else { BAD_DIRECTORY }
}

fn recover_single(text: &[u8], switch_char: u8) -> bool {
    let mut index = 0usize;
    while index < text.len() {
        let byte = text[index];
        if byte == b' ' || byte == b'\t' {
            index += 1;
            continue;
        }
        if byte != switch_char {
            return false;
        }
        index += 1;
        if index >= text.len() {
            return false;
        }
        let letter = text[index];
        let folded = if letter.is_ascii_lowercase() { letter - 0x20 } else { letter };
        return folded == b'C';
    }
    false
}

fn fresh() -> Startup {
    Startup {
        fail: 0,
        permanent: 0,
        prompt: 0xFF,
        single: 0,
        error: 0,
        messages: 0,
        paragraphs: env_paragraphs(ENV_MINIMUM),
        date_seen: 0,
        size_seen: 0,
    }
}

fn apply(items: &[Item]) -> Startup {
    let mut state = fresh();
    let mut index = 0usize;
    while index < items.len() {
        match items[index] {
            Item::Fail => {
                if state.fail != 0 {
                    state.error = TOO_MANY;
                } else {
                    state.fail = 1;
                }
            }
            Item::Permanent => {
                if state.permanent != 0 {
                    state.error = TOO_MANY;
                } else {
                    state.permanent = 1;
                    if state.prompt == 0xFF {
                        state.prompt = 0;
                    }
                }
            }
            Item::NoDate => {
                if state.date_seen != 0 {
                    state.error = TOO_MANY;
                } else {
                    state.date_seen = 1;
                    state.prompt = 1;
                }
            }
            Item::Single => {
                state.single = 1;
                state.permanent = 0;
                state.prompt = 1;
                break;
            }
            Item::Environment(bytes) => {
                if state.size_seen != 0 {
                    state.error = TOO_MANY;
                } else {
                    state.size_seen = 1;
                    state.paragraphs = env_paragraphs(bytes);
                }
            }
            Item::Messages => {
                if state.messages == RESIDENT_MESSAGES {
                    state.error = TOO_MANY;
                } else {
                    state.messages = RESIDENT_MESSAGES;
                }
            }
            Item::Invalid { text, switch_char } => {
                if recover_single(text, switch_char) {
                    state.single = 1;
                    state.permanent = 0;
                    state.prompt = 1;
                    break;
                }
                state.error = BAD_SWITCH;
            }
        }
        index += 1;
    }
    state
}

fn message_home(permanent: bool, extended: bool) -> u8 {
    if !permanent {
        if extended { MISSING } else { 0 }
    } else if extended {
        RESIDENT_MESSAGES
    } else {
        DISK_MESSAGES
    }
}

fn console(opened: bool, attributes: u8, accepted: bool) -> u8 {
    if !opened {
        return 0;
    }
    if attributes & 0x80 == 0 {
        return 0;
    }
    if !accepted {
        return 0;
    }
    1
}

fn write_state(label: &[u8], state: &Startup) {
    crate::bios::mscon::con_write_bytes(label);
    put_hex8(state.fail);
    put_hex8(state.permanent);
    put_hex8(state.prompt);
    put_hex8(state.single);
    put_hex8(state.error);
    put_hex8(state.messages);
    put_hex16(state.paragraphs);
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"INIT VR");
    put_hex8(version_fate(true, false));
    crate::bios::mscon::con_write_bytes(b" VR");
    put_hex8(version_fate(false, true));
    crate::bios::mscon::con_write_bytes(b" VR");
    put_hex8(version_fate(false, false));
    crate::bios::mscon::con_write_bytes(b" AP");
    put_hex8(u8::from(append_cleared(0, 0xFFFF)));
    crate::bios::mscon::con_write_bytes(b" AP");
    put_hex8(u8::from(append_cleared(1, 0)));
    crate::bios::mscon::con_write_bytes(b" AP");
    put_hex8(u8::from(append_cleared(1, 0xFFFF)));
    crate::bios::mscon::con_write_bytes(b" SP");
    put_hex8(separator(b'/', b'/'));
    crate::bios::mscon::con_write_bytes(b" SP");
    put_hex8(separator(b'-', b'/'));
    crate::bios::mscon::con_write_bytes(b" CD");
    put_hex8(command_unit(0));
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(drive_letter(1));
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(drive_letter(3));
    crate::bios::mscon::con_write_bytes(b" TP");
    put_hex16(low_buffer(0x2000));
    crate::bios::mscon::con_write_bytes(b" TP");
    put_hex16(low_buffer(0xF000));
    crate::bios::mscon::con_write_bytes(b" EV");
    put_hex16(env_paragraphs(ENV_MINIMUM));
    crate::bios::mscon::con_write_bytes(b" EV");
    put_hex16(env_paragraphs(161));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'/', b'/')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'/', b'\\')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'-', b'/')));
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex8(u8::from(path_char(b'-', b'A')));
    crate::bios::mscon::con_write_bytes(b" CC");
    put_hex16(comspec_count(true));
    crate::bios::mscon::con_write_bytes(b" CC");
    put_hex16(comspec_count(false));
    crate::bios::mscon::con_write_bytes(b" CM");
    put_hex16(comspec_message(65));
    crate::bios::mscon::con_write_bytes(b" CM");
    put_hex16(comspec_message(5));
    write_state(b" P", &apply(&[Item::Permanent]));
    write_state(b" D", &apply(&[Item::NoDate]));
    write_state(b" PD", &apply(&[Item::Permanent, Item::NoDate]));
    write_state(b" DP", &apply(&[Item::NoDate, Item::Permanent]));
    write_state(b" C", &apply(&[Item::Single]));
    write_state(b" F", &apply(&[Item::Fail]));
    write_state(b" FF", &apply(&[Item::Fail, Item::Fail]));
    write_state(b" M", &apply(&[Item::Messages]));
    write_state(b" MM", &apply(&[Item::Messages, Item::Messages]));
    write_state(b" E", &apply(&[Item::Environment(161)]));
    write_state(b" CP", &apply(&[Item::Single, Item::Permanent]));
    write_state(b" PC", &apply(&[Item::Permanent, Item::Single]));
    write_state(
        b" BC",
        &apply(&[Item::Invalid { text: b" /C", switch_char: b'/' }]),
    );
    write_state(
        b" BX",
        &apply(&[Item::Invalid { text: b" /X", switch_char: b'/' }]),
    );
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex8(message_home(false, false));
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex8(message_home(false, true));
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex8(message_home(true, false));
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex8(message_home(true, true));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(console(false, 0x80, true));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(console(true, 0, true));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(console(true, 0x80, false));
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(console(true, 0x80, true));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}
