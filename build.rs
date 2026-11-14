fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=/base:0x1000000");
    println!("cargo:rustc-link-arg=/export:dos_rs_bios_entry");
    build_cpu_library(std::path::Path::new(&dir));
    println!("cargo:rerun-if-env-changed=DOS_RS_EXT_MIB");
    let mib = std::env::var("DOS_RS_EXT_MIB")
        .unwrap_or_else(|_| "8".into())
        .parse::<usize>()
        .expect("DOS_RS_EXT_MIB must be an integer");
    assert!(
        (1..=128).contains(&mib),
        "per-session extended memory must be 1..128 MiB"
    );
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("guest-ram-bytes.rs"),
        ((mib + 1) * 1024 * 1024).to_string(),
    )
    .unwrap();
    println!("cargo:rerun-if-env-changed=DOS_RS_GUEST_IMAGE");
    let root = std::path::Path::new(&dir);
    let seed = root.join("disks/guest12.img");
    let image = match std::env::var_os("DOS_RS_GUEST_IMAGE") {
        Some(image) => std::path::PathBuf::from(image),
        None if std::env::var_os("CARGO_FEATURE_KERNEL_SELFTEST").is_some() => {
            let fixture = root.join("tests/fixtures/AUTOEXEC.BAT");
            println!("cargo:rerun-if-changed={}", seed.display());
            println!("cargo:rerun-if-changed={}", fixture.display());
            let image = out.join("selftest.img");
            std::fs::copy(seed, &image).unwrap();
            let batch = out.join("AUTOEXEC.BAT");
            let lines = std::fs::read_to_string(fixture).unwrap();
            std::fs::write(
                &batch,
                lines
                    .lines()
                    .map(|s| format!("{s}\r\n"))
                    .collect::<String>(),
            )
            .unwrap();
            let status = std::process::Command::new("mcopy")
                .args(["-o", "-i"])
                .arg(&image)
                .arg(batch)
                .arg("::AUTOEXEC.BAT")
                .status()
                .expect("install mtools for the self-test image");
            assert!(status.success(), "could not install the self-test batch");
            image
        }
        None => seed,
    };
    let image = image
        .canonicalize()
        .expect("cannot open DOS_RS_GUEST_IMAGE");
    println!("cargo:rustc-env=DOS_RS_GUEST_IMAGE={}", image.display());
    if !image.starts_with(&out) {
        println!("cargo:rerun-if-changed={}", image.display());
    }
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
