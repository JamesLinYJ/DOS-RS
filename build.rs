fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/link.ld");
    println!("cargo:rustc-link-arg=-zmax-page-size=0x1000");
    println!("cargo:rerun-if-changed=link.ld");
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
