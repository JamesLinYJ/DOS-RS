use crate::{put, screen, text};
include!(concat!(env!("OUT_DIR"), "/edit-messages.rs"));
pub struct Language(pub u16);
impl Language {
    pub fn message(&self, id: Message) {
        let bytes = translated(self.0, id);
        let mut i = 0;
        while i < bytes.len() {
            put(bytes[i] as u16);
            i += 1;
        }
    }
    pub fn help(&self) {
        screen(2, 0, 0);
        self.line(1, Message::Help);
        self.line(3, Message::FileKeys);
        self.line(5, Message::FindKeys);
        self.line(7, Message::MoveKeys);
        self.line(9, Message::Back);
        screen(15, 0, 0);
    }
    fn line(&self, y: u16, id: Message) {
        screen(3, 1, y);
        self.message(id);
    }
    pub fn choose(&mut self) {
        screen(2, 0, 0);
        self.line(1, Message::Language);
        if screen(7, 0, 0) == 7 {
            screen(3, 1, 3);
            text("This text adapter uses English. Graphics modes support all eight languages.");
            self.line(6, Message::Back);
            screen(15, 0, 0);
            return;
        }
        let mut i = 0;
        while i < 8 {
            screen(3, 2 + (i & 1) * 38, 3 + (i >> 1));
            put(49 + i);
            text("  ");
            Language(i).message(Message::Name);
            i += 1;
        }
        self.line(9, Message::Back);
        loop {
            let k = screen(15, 0, 0);
            if k == 27 {
                return;
            }
            if (49..=56).contains(&k) {
                self.0 = k - 49;
                return;
            }
        }
    }
}

impl crate::input::Labels for Language {
    fn pinyin(&self) {
        self.message(Message::Pinyin);
    }
    fn unavailable(&self) {
        self.message(Message::InputError);
    }
}
