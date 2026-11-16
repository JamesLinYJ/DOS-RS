// Target DOS uses GB18030, while translators edit UTF-8 JSON.
use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=edit-locales.json");
    println!("cargo:rerun-if-changed=../scripts/edit-messages.py");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let status = Command::new("python3")
        .arg("../scripts/edit-messages.py")
        .arg("edit-locales.json")
        .arg(&out)
        .status()
        .expect("compile Edit translations");
    assert!(status.success(), "invalid Edit translation catalog");
}
