fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=/base:0x1000000");
    println!("cargo:rustc-link-arg=/export:dos_rs_bios_entry");
    build_cpu_library(std::path::Path::new(&dir));
    println!("cargo:rerun-if-env-changed=DOS_RS_GUEST_IMAGE");
    let image = std::env::var_os("DOS_RS_GUEST_IMAGE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(&dir).join("disks/guest12.img"));
    let image = image
        .canonicalize()
        .expect("cannot open DOS_RS_GUEST_IMAGE");
    println!("cargo:rustc-env=DOS_RS_GUEST_IMAGE={}", image.display());
    println!("cargo:rerun-if-changed={}", image.display());
}

fn build_cpu_library(root: &std::path::Path) {
    use std::{env, path::PathBuf, process::Command};
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "uefi",
        "use an i686 or x86_64 UEFI target"
    );
    let ia32 = env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "x86";
    let (arch, format, symbols) = if ia32 {
        ("Ia32", "win32", "-DASM_PFX(x)=_ %+ x")
    } else {
        ("X64", "win64", "-DASM_PFX(x)=x")
    };
    let lib = root.join("target/edk2-cpu");
    let mp = lib.join("UefiCpuPkg/Library/MpInitLib");
    let inc = lib.join("MdePkg/Include").join(arch);
    println!("cargo:rerun-if-changed=scripts/edk2-cpu.sha256");
    for path in [
        inc.join("Nasm.inc"),
        mp.join("MpEqu.inc"),
        mp.join("X64/AmdSev.nasm"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for source in [
        mp.join(arch).join("MpFuncs.nasm"),
        lib.join("MdePkg/Library/BaseCpuLib")
            .join(arch)
            .join("InitializeFpu.nasm"),
        lib.join("UefiCpuPkg/Library/CpuExceptionHandlerLib")
            .join(arch)
            .join("ExceptionHandlerAsm.nasm"),
    ] {
        assert!(
            source.is_file(),
            "run python3 scripts/prepare-cpu.py before building"
        );
        let obj = PathBuf::from(env::var_os("OUT_DIR").unwrap())
            .join(source.file_stem().unwrap())
            .with_extension("obj");
        let status = Command::new("nasm")
            .args(["-f", format, symbols])
            .args((0..32).map(|n| format!("-DBIT{n}={}", 1u32 << n)))
            .arg(format!("-I{}/", inc.display()))
            .arg(format!("-I{}/", mp.display()))
            .arg(format!("-I{}/{arch}/", mp.display()))
            .arg(&source)
            .arg("-o")
            .arg(&obj)
            .status()
            .expect("install NASM for the upstream CPU library");
        assert!(status.success(), "could not build {}", source.display());
        println!("cargo:rerun-if-changed={}", source.display());
        println!("cargo:rustc-link-arg={}", obj.display());
    }
}
