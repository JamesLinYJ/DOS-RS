//! OPTIND holds the argument index and the offset within an option cluster.
//! Ordinary variable assignment and subshell copying preserve the entire state.
use super::{Shell, diagnostic, syntax, write_all};
use alloc::{
    format,
    string::{String, ToString},
};

struct Cursor {
    index: usize,
    offset: usize,
}

enum Event {
    Option(char, Option<String>),
    Unknown(char),
    Missing(char),
    End,
}

impl Cursor {
    fn new(value: &str) -> Self {
        let (index, offset) = value.split_once(':').unwrap_or((value, "0"));
        Self {
            index: index.parse::<usize>().unwrap_or(1).max(1),
            offset: offset.parse().unwrap_or(0),
        }
    }

    fn value(&self) -> String {
        if self.offset == 0 {
            self.index.to_string()
        } else {
            format!("{}:{}", self.index, self.offset)
        }
    }

    fn next(&mut self, options: &str, args: &[String]) -> Event {
        let Some(text) = args.get(self.index - 1) else {
            self.index = args.len() + 1;
            return Event::End;
        };
        if self.offset == 0 {
            if text == "--" {
                self.index += 1;
                return Event::End;
            }
            if !text.starts_with('-') || text.len() == 1 {
                return Event::End;
            }
            self.offset = 1;
        }
        // A caller can replace the parameters without resetting OPTIND.
        let Some(option) = text.get(self.offset..).and_then(|s| s.chars().next()) else {
            self.offset = 0;
            return Event::End;
        };
        self.offset += option.len_utf8();
        let found = options.char_indices().find(|(_, ch)| *ch == option);
        if let Some((at, _)) = found.filter(|_| option != ':' && option != '?') {
            if options[at + option.len_utf8()..].starts_with(':') {
                let argument = if self.offset < text.len() {
                    Some(text[self.offset..].to_string())
                } else {
                    let value = args.get(self.index).cloned();
                    self.index += usize::from(value.is_some());
                    value
                };
                self.index += 1;
                self.offset = 0;
                return match argument {
                    Some(value) => Event::Option(option, Some(value)),
                    None => Event::Missing(option),
                };
            }
        }
        if self.offset == text.len() {
            self.index += 1;
            self.offset = 0;
        }
        if found.is_some() && option != ':' && option != '?' {
            Event::Option(option, None)
        } else {
            Event::Unknown(option)
        }
    }
}

impl Shell {
    pub(super) fn get_options(&mut self, args: &[String]) -> i32 {
        let [options, name, rest @ ..] = args else {
            diagnostic("getopts: expected an option string and variable name");
            return 2;
        };
        if !syntax::identifier(name) {
            diagnostic("getopts: invalid variable name");
            return 2;
        }
        for name in ["OPTIND", "OPTARG", name.as_str()] {
            if self.check_variable(name).is_err() {
                return 2;
            }
        }
        let mut cursor = Cursor::new(self.get("OPTIND"));
        let parameters = if rest.is_empty() {
            &self.args[1..]
        } else {
            rest
        };
        let silent = options.starts_with(':');
        let options = options.strip_prefix(':').unwrap_or(options);
        let event = cursor.next(options, parameters);
        let (option, argument, message, status) = match event {
            Event::Option(ch, value) => (ch, value, None, 0),
            Event::End => ('?', None, None, 1),
            Event::Unknown(ch) if silent => ('?', Some(ch.to_string()), None, 0),
            Event::Missing(ch) if silent => (':', Some(ch.to_string()), None, 0),
            Event::Unknown(ch) => ('?', None, Some(format!("unknown option -{ch}")), 0),
            Event::Missing(ch) => (
                '?',
                None,
                Some(format!("option -{ch} requires an argument")),
                0,
            ),
        };
        let argument_result = if let Some(value) = argument {
            self.set("OPTARG", &value)
        } else {
            self.unset_variable("OPTARG")
        };
        if argument_result.is_err()
            || self.set("OPTIND", &cursor.value()).is_err()
            || self.set(name, &option.to_string()).is_err()
        {
            return 2;
        }
        if let Some(message) = message {
            let program = self.args.first().map_or("sh", String::as_str);
            let _ = write_all(2, format!("{program}: getopts: {message}\n").as_bytes());
        }
        status
    }
}
