// Generate the kernel dispatch from the same ABI table used by native-cc.py.
use std::{env, fmt::Write, fs, path::PathBuf};

fn main() {
    let table = "libc/syscalls.def";
    println!("cargo:rerun-if-changed={table}");
    let entries = fs::read_to_string(table).unwrap();
    let mut variants = String::new();
    let mut dispatch = String::new();
    for row in entries.lines() {
        let fields: Vec<_> = row.split_whitespace().collect();
        let [name, number, arguments] = fields[..] else {
            panic!("invalid syscall row: {row}")
        };
        let variant = name[..1].to_uppercase() + &name[1..];
        writeln!(variants, "{variant},").unwrap();
        writeln!(dispatch, "{number} => Some((Self::{variant},{arguments})),").unwrap();
    }
    let source = format!(
        "#[derive(Clone, Copy, PartialEq, Eq)] pub enum Call {{ {variants} }}
         impl Call {{ pub fn decode(id: usize) -> Option<(Self, usize)> {{
              match id {{ {dispatch} _ => None }}
         }} }}"
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("calls.rs"),
        source,
    )
    .unwrap();
}
