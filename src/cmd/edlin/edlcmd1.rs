//! Line editing from `CMD/EDLIN/EDLCMD1.ASM`.
//!
//! Append accepts one parameter. A missing count loads until the buffer is
//! three quarters full, and a short read is an end of file only when one
//! more byte cannot be read. Write accepts at most one parameter and closes
//! the file when the disk cannot take the whole span. Replace and search
//! accept at most two parameters. Delete, page, and list do too. A bare
//! line number accepts fewer than two.

const READ_FILE: u8 = 0x3F;
const WRITE_FILE: u8 = 0x40;
const MOVE_POINTER: u8 = 0x42;
const CLOSE_FILE: u8 = 0x3E;
const EXIT_CALL: u8 = 0x4C;
const BUFFERED_INPUT: u8 = 0x0A;
const LINE_LIMIT: u16 = 254;
const RELATIVE: u8 = 1;
const SEEK_BACK: u16 = 0xFFFF;
const FAIL_CODE: u8 = 0xFF;

enum Page {
    Error,
    Quiet,
    Lines(u16),
}

enum Move {
    Missing,
    Error,
    Count(u16),
}

fn append_gate(count: u16, at_end: bool, param: u16, filled: u16, mark: u16) -> u8 {
    if count != 1 {
        1
    } else if at_end {
        2
    } else if param == 0 && filled >= mark {
        3
    } else {
        0
    }
}

fn room(last: u16, end: u16) -> Option<u16> {
    if last == end { None } else { Some(last.wrapping_sub(end)) }
}

fn read_count(asked: u16, got: u16, extra: u16) -> (u16, bool) {
    if got == asked {
        (got, false)
    } else if extra == 0 {
        (got, true)
    } else {
        (got.wrapping_add(1), false)
    }
}

fn saw_marker(already: bool, found: bool) -> bool {
    found || already
}

fn lines_requested(param: u16, end: u16, count: u16, mark: u16) -> (u16, u16, u16) {
    if param != 0 {
        (end, param, 0)
    } else {
        let after = end.wrapping_add(count);
        if after <= mark {
            (end, 0, 0)
        } else {
            (mark, 1, after.wrapping_sub(mark))
        }
    }
}

fn finish_line(complete: bool, at_end: bool) -> u8 {
    if complete {
        0
    } else if at_end {
        2
    } else {
        1
    }
}

fn rewind(unused: u16) -> Option<u16> {
    if unused == 0 { None } else { Some(unused) }
}

fn after_lines(wanted: u16, found: u16, at_end: bool, ending: bool) -> u8 {
    if wanted == found {
        0
    } else if at_end {
        1
    } else if ending {
        2
    } else {
        3
    }
}

fn write_ok(count: u16) -> bool {
    count <= 1
}

fn front_point(end: u16, quarter: u16, start: u16) -> Option<u16> {
    if end <= quarter {
        return None;
    }
    let point = end.wrapping_sub(quarter);
    if point <= start { None } else { Some(point) }
}

fn write_amount(param: u16, end: u16, quarter: u16, start: u16, until: u16) -> Option<u16> {
    if param == 0 && front_point(end, quarter, start).is_none() {
        return None;
    }
    let amount = until.wrapping_sub(start);
    if amount == 0 { None } else { Some(amount) }
}

fn removes_backup(flag: u8) -> bool {
    flag == 0
}

fn stored(amount: u16, carried: bool, written: u16) -> bool {
    !carried && written == amount
}

fn replace_mode(from_current: bool, count: u16) -> Result<u8, ()> {
    if count > 2 { Err(()) } else { Ok(u8::from(from_current)) }
}

fn replaced_length(line: u16, old: u16, new: u16) -> Option<u16> {
    let len = line.wrapping_sub(old).wrapping_add(new);
    if len <= LINE_LIMIT { Some(len) } else { None }
}

fn remaining_search(left: u16, old: u16) -> u16 {
    let drop = old.wrapping_sub(1);
    if left >= drop { left.wrapping_sub(drop) } else { 0 }
}

fn copied(count: u16, remaining: u16) -> (u16, u16) {
    if count == 0 { (0, remaining) } else { (count, remaining.wrapping_sub(count)) }
}

fn search_mode(from_current: bool, count: u16) -> Result<u8, ()> {
    if count > 2 { Err(()) } else { Ok(u8::from(from_current)) }
}

fn current_line(last_number: u16) -> u16 {
    last_number.wrapping_sub(1)
}

fn delete_bounds(count: u16, first: u16, second: u16, current: u16) -> Result<(u16, u16), ()> {
    if count > 2 {
        return Err(());
    }
    let start = if first == 0 { current } else { first };
    let end = if second == 0 { start } else { second };
    Ok((start, end))
}

fn moved(found: bool, end: u16, source: u16) -> Move {
    if !found {
        Move::Missing
    } else if end < source {
        Move::Error
    } else {
        Move::Count(end.wrapping_sub(source).wrapping_add(1))
    }
}

fn page_start(param: u16, current: u16) -> u16 {
    if param != 0 {
        param
    } else if current == 1 {
        current
    } else {
        current.wrapping_add(1)
    }
}

fn page_span(count: u16, first: u16, second: u16, current: u16, last: u16, screen: u8) -> Page {
    if count > 2 {
        return Page::Error;
    }
    let start = page_start(first, current);
    if start > last {
        return Page::Quiet;
    }
    let mut end = if second != 0 {
        second
    } else {
        start.wrapping_add(u16::from(screen.wrapping_sub(2)))
    };
    end = end.wrapping_add(1);
    if end > last {
        end = last;
    }
    if end == start {
        Page::Quiet
    } else if end < start {
        Page::Error
    } else {
        Page::Lines(end.wrapping_sub(start))
    }
}

fn list_start(param: u16, current: u16) -> u16 {
    if param != 0 {
        param
    } else if current > 11 {
        current.wrapping_sub(11)
    } else {
        1
    }
}

fn list_span(count: u16, first: u16, second: u16, current: u16, screen: u8, found: bool) -> Result<u16, bool> {
    if count > 2 {
        return Err(true);
    }
    if !found {
        return Err(false);
    }
    let start = list_start(first, current);
    let next = second.wrapping_add(1);
    if next > start {
        Ok(next.wrapping_sub(start))
    } else {
        Ok(u16::from(screen))
    }
}

fn edit_line(count: u16, param: u16, current: u16) -> Result<u16, ()> {
    if count >= 2 {
        Err(())
    } else if param == 0 {
        Ok(current.wrapping_add(1))
    } else {
        Ok(param)
    }
}

fn edit_input(found: bool, at_end: bool, typed: u8) -> u8 {
    if !found {
        0
    } else if at_end {
        1
    } else if typed == 0 {
        2
    } else {
        3
    }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"EDLCMD1 RD");
    put_hex8(READ_FILE);
    crate::bios::mscon::con_write_bytes(b" WR");
    put_hex8(WRITE_FILE);
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(MOVE_POINTER);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(CLOSE_FILE);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(EXIT_CALL);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(BUFFERED_INPUT);
    write_gate(append_gate(2, false, 0, 0, 0));
    write_gate(append_gate(1, true, 1, 0, 0));
    write_gate(append_gate(1, false, 0, 200, 200));
    write_gate(append_gate(1, false, 0, 199, 200));
    write_gate(append_gate(1, false, 5, 200, 200));
    write_gate(append_gate(0, false, 0, 0, 0));
    write_room(room(100, 100));
    write_room(room(180, 100));
    write_read(read_count(10, 10, 1));
    write_read(read_count(10, 4, 0));
    write_read(read_count(10, 4, 1));
    write_flag(saw_marker(false, true));
    write_flag(saw_marker(true, false));
    write_flag(saw_marker(false, false));
    write_lines(lines_requested(5, 0x0100, 10, 0x0180));
    write_lines(lines_requested(0, 0x0100, 0x0020, 0x0180));
    write_lines(lines_requested(0, 0x0100, 0x0100, 0x0180));
    write_finish(finish_line(true, false));
    write_finish(finish_line(false, true));
    write_finish(finish_line(false, false));
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex8(RELATIVE);
    crate::bios::mscon::con_write_bytes(b" CX");
    put_hex16(SEEK_BACK);
    write_rewind(rewind(0));
    write_rewind(rewind(4));
    write_after(after_lines(3, 3, true, false));
    write_after(after_lines(3, 1, true, false));
    write_after(after_lines(3, 1, false, true));
    write_after(after_lines(3, 1, false, false));
    crate::bios::mscon::con_write_bytes(if write_ok(2) { b" OK" } else { b" CE" });
    crate::bios::mscon::con_write_bytes(if write_ok(1) { b" OK" } else { b" CE" });
    crate::bios::mscon::con_write_bytes(if write_ok(0) { b" OK" } else { b" CE" });
    write_point(front_point(100, 40, 20));
    write_point(front_point(30, 40, 20));
    write_point(front_point(50, 40, 20));
    write_point(write_amount(0, 30, 40, 20, 0x0080));
    write_point(write_amount(4, 100, 40, 0x0040, 0x0080));
    write_point(write_amount(4, 100, 40, 0x0040, 0x0040));
    crate::bios::mscon::con_write_bytes(if removes_backup(0) { b" DB" } else { b" KB" });
    crate::bios::mscon::con_write_bytes(if removes_backup(1) { b" DB" } else { b" KB" });
    write_stored(stored(10, false, 10));
    write_stored(stored(10, true, 10));
    write_stored(stored(10, false, 9));
    write_mode(replace_mode(true, 2));
    write_mode(replace_mode(true, 3));
    write_mode(replace_mode(false, 2));
    write_len(replaced_length(20, 4, 6));
    write_len(replaced_length(20, 4, 250));
    write_len(replaced_length(10, 4, 248));
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex16(remaining_search(10, 4));
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex16(remaining_search(2, 4));
    let (count, left) = copied(3, 10);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(count);
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex16(left);
    let (count, left) = copied(0, 10);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(count);
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex16(left);
    crate::bios::mscon::con_write_bytes(b" AC SK");
    write_mode(search_mode(true, 1));
    write_mode(search_mode(true, 3));
    write_mode(search_mode(false, 2));
    crate::bios::mscon::con_write_bytes(b" FN AC CU");
    put_hex16(current_line(8));
    crate::bios::mscon::con_write_bytes(b" NF");
    write_bounds(delete_bounds(3, 1, 2, 4));
    write_bounds(delete_bounds(2, 0, 0, 4));
    write_bounds(delete_bounds(1, 2, 0, 9));
    write_bounds(delete_bounds(2, 3, 8, 1));
    write_move(moved(false, 100, 80));
    write_move(moved(true, 100, 80));
    write_move(moved(true, 50, 80));
    write_page(page_span(3, 0, 0, 1, 40, 25));
    write_start(page_start(0, 1));
    write_start(page_start(0, 5));
    write_start(page_start(9, 5));
    write_page(page_span(2, 10, 0, 1, 8, 25));
    write_page(page_span(2, 10, 0, 1, 40, 25));
    write_page(page_span(2, 10, 30, 1, 40, 25));
    write_page(page_span(2, 30, 0, 1, 40, 25));
    write_page(page_span(2, 40, 0, 1, 40, 25));
    write_page(page_span(2, 10, 5, 1, 40, 25));
    crate::bios::mscon::con_write_bytes(if list_span(3, 0, 0, 1, 25, true).is_err() { b" CE" } else { b" OK" });
    write_start(list_start(0, 13));
    write_start(list_start(0, 12));
    write_start(list_start(0, 1));
    write_start(list_start(4, 1));
    write_list(list_span(2, 5, 20, 1, 25, false));
    write_list(list_span(2, 5, 20, 1, 25, true));
    write_list(list_span(2, 5, 0, 1, 25, true));
    write_list(list_span(2, 5, 4, 1, 25, true));
    write_edit(edit_line(2, 0, 4));
    write_edit(edit_line(1, 0, 4));
    write_edit(edit_line(0, 7, 4));
    write_typed(edit_input(false, false, 3));
    write_typed(edit_input(true, true, 3));
    write_typed(edit_input(true, false, 0));
    write_typed(edit_input(true, false, 3));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_gate(kind: u8) {
    let tag: &[u8] = match kind {
        0 => b" LD",
        2 => b" NM",
        3 => b" FL",
        _ => b" CE",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_room(bytes: Option<u16>) {
    match bytes {
        None => crate::bios::mscon::con_write_bytes(b" ME"),
        Some(bytes) => {
            crate::bios::mscon::con_write_bytes(b" AV");
            put_hex16(bytes);
        }
    }
}

fn write_read(read: (u16, bool)) {
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex16(read.0);
    crate::bios::mscon::con_write_bytes(if read.1 { b" EF01" } else { b" EF00" });
}

fn write_flag(found: bool) {
    crate::bios::mscon::con_write_bytes(if found { b" EF01" } else { b" EF00" });
}

fn write_lines(request: (u16, u16, u16)) {
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex16(request.0);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex16(request.1);
    crate::bios::mscon::con_write_bytes(b" LV");
    put_hex16(request.2);
}

fn write_finish(kind: u8) {
    let tag: &[u8] = match kind {
        0 => b" KP",
        2 => b" AD02",
        _ => b" BK",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_rewind(unused: Option<u16>) {
    match unused {
        None => crate::bios::mscon::con_write_bytes(b" NS"),
        Some(unused) => {
            crate::bios::mscon::con_write_bytes(b" RL");
            put_hex16(unused);
        }
    }
}

fn write_after(kind: u8) {
    let tag: &[u8] = match kind {
        0 => b" CL",
        1 => b" NM",
        2 => b" SP",
        _ => b" ME",
    };
    crate::bios::mscon::con_write_bytes(tag);
}

fn write_point(point: Option<u16>) {
    match point {
        None => crate::bios::mscon::con_write_bytes(b" EM"),
        Some(point) => {
            crate::bios::mscon::con_write_bytes(b" PT");
            put_hex16(point);
        }
    }
}

fn write_stored(full: bool) {
    if full {
        crate::bios::mscon::con_write_bytes(b" OK");
    } else {
        crate::bios::mscon::con_write_bytes(b" CL AL");
        put_hex8(FAIL_CODE);
    }
}

fn write_mode(mode: Result<u8, ()>) {
    match mode {
        Err(()) => crate::bios::mscon::con_write_bytes(b" CE"),
        Ok(mode) => {
            crate::bios::mscon::con_write_bytes(b" MD");
            put_hex8(mode);
        }
    }
}

fn write_len(len: Option<u16>) {
    match len {
        None => crate::bios::mscon::con_write_bytes(b" TL"),
        Some(len) => {
            crate::bios::mscon::con_write_bytes(b" LN");
            put_hex16(len);
        }
    }
}

fn write_bounds(bounds: Result<(u16, u16), ()>) {
    match bounds {
        Err(()) => crate::bios::mscon::con_write_bytes(b" CE"),
        Ok((start, end)) => {
            crate::bios::mscon::con_write_bytes(b" A");
            put_hex16(start);
            crate::bios::mscon::con_write_bytes(b" B");
            put_hex16(end);
        }
    }
}

fn write_move(moved: Move) {
    match moved {
        Move::Missing => crate::bios::mscon::con_write_bytes(b" NF"),
        Move::Error => crate::bios::mscon::con_write_bytes(b" CE"),
        Move::Count(count) => {
            crate::bios::mscon::con_write_bytes(b" MV");
            put_hex16(count);
        }
    }
}

fn write_start(line: u16) {
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(line);
}

fn write_page(page: Page) {
    match page {
        Page::Error => crate::bios::mscon::con_write_bytes(b" CE"),
        Page::Quiet => crate::bios::mscon::con_write_bytes(b" QT"),
        Page::Lines(count) => {
            crate::bios::mscon::con_write_bytes(b" PG");
            put_hex16(count);
        }
    }
}

fn write_list(span: Result<u16, bool>) {
    match span {
        Err(true) => crate::bios::mscon::con_write_bytes(b" CE"),
        Err(false) => crate::bios::mscon::con_write_bytes(b" NF"),
        Ok(count) => {
            crate::bios::mscon::con_write_bytes(b" PG");
            put_hex16(count);
        }
    }
}

fn write_edit(line: Result<u16, ()>) {
    match line {
        Err(()) => crate::bios::mscon::con_write_bytes(b" CE"),
        Ok(line) => {
            crate::bios::mscon::con_write_bytes(b" LN");
            put_hex16(line);
        }
    }
}

fn write_typed(kind: u8) {
    let tag: &[u8] = match kind {
        0 => b" NF",
        1 => b" EN",
        2 => b" EM",
        _ => b" RP",
    };
    crate::bios::mscon::con_write_bytes(tag);
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
