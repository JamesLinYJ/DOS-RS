//! Initialization data from `CMD/COMMAND/UINIT.ASM`.
//!
//! The console device is `/DEV/CON` with room for an eight-character name.
//! Date and time are not prompted for. `/E` accepts a number from 160 through
//! 32768. The parse block has one optional filespec and six switches, in the
//! order `/P`, `/F`, `/D`, `/E`, `/C`, `/MSG`. The append-parse pointer and
//! the end of init stay one dword.

const ENV_SMALL: u32 = 160;
const ENV_BIG: u32 = 32768;
const FILE_SPEC: u16 = 0x0200;
const OPTIONAL: u16 = 0x0001;
const NUMERIC: u16 = 0x8000;
const CAP_FILE: u16 = 1;
const CAP_CHAR: u16 = 2;

struct Switch {
    text: &'static [u8],
    kind: u16,
    function: u16,
}

fn environment_size(value: u32) -> bool {
    (ENV_SMALL..=ENV_BIG).contains(&value)
}

fn write_bytes(label: &[u8], bytes: &[u8]) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < bytes.len() {
        put_hex8(bytes[index]);
        index += 1;
    }
}

pub fn show() {
    let switches = [
        Switch { text: b"/P", kind: 0, function: CAP_CHAR },
        Switch { text: b"/F", kind: 0, function: CAP_CHAR },
        Switch { text: b"/D", kind: 0, function: CAP_CHAR },
        Switch { text: b"/E", kind: NUMERIC, function: 0 },
        Switch { text: b"/C", kind: 0, function: CAP_CHAR },
        Switch { text: b"/MSG", kind: 0, function: CAP_CHAR },
    ];
    crate::bios::mscon::con_write_bytes(b"UINIT");
    write_bytes(b" DV", b"/DEV/CON\0\0\0\0\0\0");
    write_bytes(b" CS", b"/COMMAND.COM\0\0");
    write_bytes(b" AB", b"\0:\\AUTOEXEC.BAT\0\r");
    write_bytes(b" KB", b"\0:\\KAUTOEXE.BAT\0\r");
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex8(0xFF);
    write_bytes(b" CM", b"COMSPEC=");
    write_bytes(b" CH", b"=az/\\ CA");
    crate::bios::mscon::con_write_bytes(b" EV");
    put_hex16(ENV_SMALL as u16);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex16(ENV_BIG as u16);
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(u8::from(environment_size(159)));
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(u8::from(environment_size(160)));
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(u8::from(environment_size(32768)));
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex8(u8::from(environment_size(32769)));
    crate::bios::mscon::con_write_bytes(b" PM00 PX01 SW");
    put_hex8(switches.len() as u8);
    crate::bios::mscon::con_write_bytes(b" KW00 FI");
    put_hex16(FILE_SPEC | OPTIONAL);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(CAP_FILE);
    let mut index = 0usize;
    while index < switches.len() {
        let switch = &switches[index];
        crate::bios::mscon::con_write_bytes(b" S");
        let mut byte = 0usize;
        while byte < switch.text.len() {
            put_hex8(switch.text[byte]);
            byte += 1;
        }
        put_hex16(switch.kind);
        put_hex16(switch.function);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" SG0000 BF00 CH00 RS00 XM00 EW00 DW00\r\n");
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
