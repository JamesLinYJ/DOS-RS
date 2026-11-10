//! Uninitialized transient data from `CMD/COMMAND/TSPC.ASM`.
//!
//! The bytes are laid out in source order. Several names are one location:
//! the read-EOF flag is the `IF` flag, concatenation is the first parameter,
//! and the destination FCB is the destination directory. The code-page
//! attribute is a two-byte name `CP` with a system binary value. The IBM
//! build keeps the ROM call bytes. Each parse result block is eight bytes.

const DIR_LIMIT: usize = 64 + 3;
const COMMAND_LIMIT: usize = 128;
const NAME_EXTRA: usize = 20;
const PAGE_LINES: u16 = 25;
const IOCTL_LENGTH: u16 = 14;
const ARGUMENT_LIMIT: usize = 64;
const ARGUMENT_WIDE: usize = 11;
const FIND_NAME: usize = 13;

struct Cursor {
    at: usize,
}

impl Cursor {
    fn take(&mut self, size: usize) -> usize {
        let start = self.at;
        self.at += size;
        start
    }
}

fn layout() -> Cursor {
    let mut place = Cursor { at: 0 };
    let _source_name = place.take(DIR_LIMIT + NAME_EXTRA);
    let _target_name = place.take(DIR_LIMIT + NAME_EXTRA);
    let _raw_command = place.take(COMMAND_LIMIT + 3);
    let cooked = place.take(COMMAND_LIMIT + 3);
    crate::bios::mscon::con_write_bytes(b" CM");
    put_hex16(cooked as u16);
    let _user_dir = place.take(DIR_LIMIT + 3);
    let _exec_path = place.take(COMMAND_LIMIT + 3);
    let redirect = place.take(DIR_LIMIT + 3 + FIND_NAME);
    crate::bios::mscon::con_write_bytes(b" RE");
    put_hex16(redirect as u16);
    let _head = place.take(2);
    let _resident = place.take(2);
    let _tpa = place.take(2);
    let _switch = place.take(1);
    let _dir_char = place.take(1);
    let _exec = place.take(4);
    let _recheck = place.take(4);
    let _test = place.take(2);
    let _transient = place.take(2);
    let _check_drive = place.take(1);
    let file_type = place.take(1);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex16(file_type as u16);
    let _current = place.take(1);
    let _concat_attr = place.take(1);
    let first_parm = place.take(1);
    crate::bios::mscon::con_write_bytes(b" P1");
    put_hex16(first_parm as u16);
    let second_parm = place.take(1);
    crate::bios::mscon::con_write_bytes(b" P2");
    put_hex16(second_parm as u16);
    let _command_switches = place.take(2);
    let _first_switches = place.take(2);
    let second_switches = place.take(2);
    crate::bios::mscon::con_write_bytes(b" DS");
    put_hex16(second_switches as u16);
    let all_switches = place.take(2);
    crate::bios::mscon::con_write_bytes(b" AW");
    put_hex16(all_switches as u16);
    let _copy_flag = place.take(1);
    let special_drive = place.take(1);
    crate::bios::mscon::con_write_bytes(b" DC");
    put_hex16(special_drive as u16);
    let _byte_count = place.take(2);
    let _next = place.take(2);
    let _first_search = place.take(1);
    let _line_count = place.take(1);
    let _line_len = place.take(1);
    let _file_count = place.take(2);
    let _char_buf = place.take(80);
    let id_len = place.take(1);
    crate::bios::mscon::con_write_bytes(b" F2");
    put_hex16(id_len as u16);
    let _id = place.take(8);
    let _extension = place.take(3);
    let _dest_text = place.take(37);
    let _dest_name = place.take(11);
    let dest_dir = place.take(DIR_LIMIT);
    crate::bios::mscon::con_write_bytes(b" DF");
    put_hex16(dest_dir as u16);
    let dir_buf = place.take(DIR_LIMIT + 3);
    crate::bios::mscon::con_write_bytes(b" DB");
    put_hex16(dir_buf as u16);
    let _short_dir = place.take(12);
    let _bits = place.take(2);
    let _path_count = place.take(2);
    let _path_pos = place.take(2);
    let _path_switches = place.take(2);
    let _full = place.take(2);
    let _comma = place.take(1);
    let _plus_comma = place.take(1);
    let _dir_flag = place.take(1);
    let _parse_last = place.take(2);
    let _code_page = place.take(2);
    let _attr_size = place.take(2);
    let _attr_segment = place.take(2);
    let _open_off = place.take(2);
    let _open_seg = place.take(2);
    let _open_end = place.take(2);
    let attribute = place.take(2 + 1 + 2 + 1 + 1 + 2 + 2 + 2);
    crate::bios::mscon::con_write_bytes(b" XA");
    put_hex16(attribute as u16);
    let _arg_buf = place.take(128);
    let _size_low = place.take(2);
    let _size_high = place.take(2);
    let _string_slot = place.take(2);
    let _copy_count = place.take(2);
    let _copy_flag = place.take(1);
    let _dir_count = place.take(2);
    let _bytes_free = place.take(4);
    let _major = place.take(2);
    let _minor = place.take(2);
    let _one_char = place.take(2);
    let _volume_drive = place.take(1);
    let rom_call = place.take(1);
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex16(rom_call as u16);
    let _rom_ip = place.take(2);
    let _rom_cs = place.take(2);
    let dest_vars = place.take(1 + 1 + 2 + 1 + DIR_LIMIT + NAME_EXTRA + 2 + 1 + 1 + 1 + 2);
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(dest_vars as u16);
    let source_vars = place.take(1 + 1 + 2 + 1 + DIR_LIMIT + NAME_EXTRA);
    crate::bios::mscon::con_write_bytes(b" SV");
    put_hex16(source_vars as u16);
    let _source_hand = place.take(2);
    let _source_dev = place.take(1);
    let _scan = place.take(DIR_LIMIT + NAME_EXTRA);
    let _source_point = place.take(2);
    let _inexact = place.take(1);
    let _no_write = place.take(1);
    let _binary = place.take(1);
    let _written = place.take(2);
    let _term = place.take(1);
    let _ascii = place.take(1);
    let _plus = place.take(1);
    let _objects = place.take(1);
    let _copy_date = place.take(2);
    let _copy_time = place.take(2);
    let _batch_hand = place.take(2);
    let _start_el = place.take(2);
    let _el_count = place.take(1);
    let _el_pos = place.take(1);
    let _skip = place.take(1);
    let _source = place.take(11);
    let _extension_entered = place.take(1);
    let _ioctl = place.take(1 + 1 + 2 + 2);
    let _mode = place.take(1 + 1 + 2 + 2 + 2);
    let _width = place.take(2);
    let _lines = place.take(2);
    let _volume = place.take(2 + 4 + 11 + 8);
    let _star = place.take(1);
    let _comspec = place.take(1);
    let _message_flag = place.take(1);
    let _message_number = place.take(2);
    let _append = place.take(1);
    let _print_error = place.take(2);
    let _subst = place.take(11 * 2);
    let _lead = place.take(1);
    let arg_start = place.take(ARGUMENT_LIMIT * ARGUMENT_WIDE + 2 + 2 + 256 * 2 + COMMAND_LIMIT);
    crate::bios::mscon::con_write_bytes(b" AR");
    put_hex16(arg_start as u16);
    let _arg_ptr = place.take(2);
    let _temp = place.take(128);
    let _last_arg = place.take(2);
    let _command_ptr = place.take(2);
    let find_start = place.take(1 + 11 + 1 + 2 + 2 + 4 + 1 + 2 + 2 + 2 + 2 + FIND_NAME);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex16(find_start as u16);
    let _path_info = place.take(6);
    let _separator = place.take(1);
    let _best = place.take(1);
    let _best_name = place.take(FIND_NAME);
    let _current_dir = place.take(64);
    let _search_error = place.take(2);
    let _if_count = place.take(2);
    let _mark = place.take(1);
    let _stack = place.take(0x80 * 2);
    let _country = place.take(24);
    let _country_pad = place.take(8);
    let _batch_pos = place.take(2);
    let _batch = place.take(32);
    let _batch_end = place.take(2);
    let parse_out = place.take(8);
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex16(parse_out as u16);
    let date_out = place.take(8);
    crate::bios::mscon::con_write_bytes(b" DT");
    put_hex16(date_out as u16);
    let time_out = place.take(8);
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex16(time_out as u16);
    let drive_out = place.take(8);
    crate::bios::mscon::con_write_bytes(b" DO");
    put_hex16(drive_out as u16);
    place
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"TSPC NX");
    put_hex16((DIR_LIMIT + NAME_EXTRA) as u16);
    let end = layout();
    crate::bios::mscon::con_write_bytes(b" XL");
    put_hex16(13);
    crate::bios::mscon::con_write_bytes(b" NM4350 FG8000 TY02 VL0002 AT0000 IL");
    put_hex16(IOCTL_LENGTH);
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(PAGE_LINES);
    crate::bios::mscon::con_write_bytes(b" LB000B FS0008 SB");
    put_hex16((11 * 2) as u16);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(0x80 * 2);
    crate::bios::mscon::con_write_bytes(b" FB");
    put_hex16((1 + 11 + 1 + 2 + 2 + 4 + 1 + 2 + 2 + 2 + 2 + FIND_NAME) as u16);
    crate::bios::mscon::con_write_bytes(b" EN");
    put_hex16(end.at as u16);
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
