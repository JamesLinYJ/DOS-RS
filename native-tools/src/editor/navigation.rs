// Copyright (c) Microsoft Corporation. Licensed under the MIT License.
// Word navigation from crates/edit/src/buffer/navigation.rs, over DOS character units.
use super::{Document, previous, unit};
fn class(c: u8) -> u16 {
    if c == 10 || c == 13 {
        return 1;
    }
    if c == 32 || c == 9 {
        return 2;
    }
    let separators = b"`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";
    let mut i = 0;
    while i < separators.len() {
        if c == separators[i] {
            return 3;
        }
        i += 1
    }
    4
}
pub fn word(d: &Document, mut p: usize, forward: bool) -> usize {
    let mut skip_line = true;
    let mut stage = 0;
    let mut target = 0;
    let mut seen = 0;
    loop {
        if forward && p == d.text_length || !forward && p == 0 {
            return p;
        }
        let at = if forward { p } else { previous(d, p) };
        let next = if forward { at + unit(d, at) } else { at };
        let c = class(d.byte(at));
        if skip_line && c == 1 {
            skip_line = false;
            p = next;
            continue;
        }
        skip_line = false;
        if stage == 0 && c == 2 {
            p = next;
            continue;
        }
        if stage == 0 {
            if c < 3 {
                return p;
            }
            stage = 1;
            target = c
        }
        if target != c {
            if target == 3 && seen == 1 && c == 4 {
                target = 4
            } else {
                return p;
            }
        }
        seen += 1;
        p = next;
    }
}
