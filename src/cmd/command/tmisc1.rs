//! `findcom` from `CMD/COMMAND/TMISC1.ASM`.
//!
//! The scan walks `COMTAB` until the name length and the bytes match.
//! `VER` has no switch bit, so a `/` in the tail is rejected. The command
//! that matches is the only one run.

use super::tdata::{COMTAB, F_SWITCH_ALLOWED};
use super::{tbatch, tcmd1a, tcmd1b, tcmd2a, tenv, tpipe, tucode};

impl tenv::Environment {
    pub fn dispatch(&mut self, line: &[u8]) -> bool {
        let (name, rest) = split_name(line);
        let Some(&(command, flags)) = COMTAB
            .iter()
            .find(|(entry, _)| entry.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        if flags & F_SWITCH_ALLOWED == 0 && rest.contains(&b'/') {
            return false;
        }
        match command {
            b"VER" => tcmd2a::version(),
            b"PATH" => self.path_cmd(rest),
            b"CLS" => tcmd2a::cls(),
            b"ECHO" => tucode::echo(&mut self.echo, rest),
            b"BREAK" => tucode::break_cmd(rest),
            b"VERIFY" => tucode::verify_cmd(rest),
            b"IF" => tbatch::if_cmd(self, rest),
            b"CALL" => tbatch::call_cmd(rest),
            b"GOTO" => tbatch::goto_cmd(rest),
            b"FOR" => tbatch::for_cmd(self, rest),
            b"SHIFT" => tbatch::shift_cmd(rest),
            b"PROMPT" => self.prompt_cmd(rest),
            b"SET" => self.set_cmd(rest),
            b"DATE" => tpipe::date_cmd(rest),
            b"TIME" => tpipe::time_cmd(rest),
            b"TYPE" => tcmd1b::type_cmd(rest),
            b"MD" | b"MKDIR" => tcmd1a::mkdir_cmd(rest),
            b"CD" | b"CHDIR" => tcmd1a::chdir_cmd(rest),
            b"RD" | b"RMDIR" => tcmd1a::rmdir_cmd(rest),
            b"COPY" => tcmd1a::copy_cmd(rest),
            b"REN" | b"RENAME" => tcmd1a::rename_cmd(rest),
            b"DEL" | b"ERASE" => tcmd1a::erase_cmd(rest),
            b"DIR" => tcmd1a::dir_cmd(rest),
            b"ATTRIB" => tcmd1a::attrib_cmd(rest),
            b"VOL" => tcmd1b::volume(rest),
            _ => return false,
        }
        true
    }
}

fn split_name(line: &[u8]) -> (&[u8], &[u8]) {
    let line = line.trim_ascii_start();
    let end = line
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(line.len());
    line.split_at(end)
}
