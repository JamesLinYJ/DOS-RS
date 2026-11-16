use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let cpu = root.join("../../target/user32-cpu");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let vectors = Command::new("perl")
        .arg(cpu.join("vectors.pl"))
        .output()
        .unwrap();
    assert!(
        vectors.status.success(),
        "prepare the pinned user32 CPU library first"
    );
    fs::write(out.join("vectors.S"), vectors.stdout).unwrap();
    for source in [
        out.join("vectors.S"),
        cpu.join("trapasm.S"),
        cpu.join("swtch.S"),
    ] {
        let obj = out.join(source.file_stem().unwrap()).with_extension("o");
        assert!(
            Command::new("i686-elf-gcc")
                .args(["-m32", "-march=i386", "-c"])
                .arg(&source)
                .arg("-o")
                .arg(&obj)
                .status()
                .unwrap()
                .success()
        );
        println!("cargo:rustc-link-arg={}", obj.display());
    }
    for file in ["vectors.pl", "trapasm.S", "swtch.S", "mmu.h"] {
        println!("cargo:rerun-if-changed={}", cpu.join(file).display());
    }
    println!("cargo:rerun-if-changed=layout.ld");
    println!(
        "cargo:rustc-link-arg=-T{}",
        root.join("layout.ld").display()
    );
}
