use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bios = root.join("target/bios-cpu");
    println!("cargo:rustc-env=BIOS_CPU={}", bios.display());
    // Keep the pinned upstream BIOS register/interrupt sequence, and decode
    // its actual returned byte count before handing descriptors to either PE.
    let upstream = bios.join("bios/stage-2/src/memory_map.rs");
    let patch = root.join("boot/bios/e820.patch");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bios-memory-map.rs");
    assert!(
        Command::new("patch")
            .args(["-s", "-F", "0", "-o"])
            .arg(out)
            .arg(&upstream)
            .arg(&patch)
            .status()
            .expect("applying the BIOS E820 adapter")
            .success()
    );
    println!("cargo:rerun-if-changed={}", patch.display());
    println!("cargo:rerun-if-changed={}", upstream.display());
    for (bin, path) in [
        ("mbr", bios.join("bios/boot_sector/boot-sector-link.ld")),
        ("loader", root.join("boot/bios/loader.ld")),
        ("enter", root.join("boot/bios/enter.ld")),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rustc-link-arg-bin={bin}=--script={}", path.display());
    }
    let lib = root.join("target/edk2-cpu");
    for func in ["EnablePaging64", "InternalSwitchStack", "InitializeFpu"] {
        let source = lib
            .join(if func == "InitializeFpu" {
                "MdePkg/Library/BaseCpuLib/Ia32"
            } else {
                "MdePkg/Library/BaseLib/Ia32"
            })
            .join(format!("{func}.nasm"));
        let obj = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(format!("{func}.o"));
        assert!(
            Command::new("nasm")
                .args(["-f", "elf32", "-DASM_PFX(x)=x", "-DBIT9=512"])
                .arg(&source)
                .arg("-o")
                .arg(&obj)
                .status()
                .unwrap()
                .success()
        );
        println!("cargo:rerun-if-changed={}", source.display());
        println!("cargo:rustc-link-arg-bin=enter={}", obj.display());
    }
}
