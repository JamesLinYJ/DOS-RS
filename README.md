# dos4-rs

`dos4-rs` is a Rust kernel for `i686-unknown-none` and `x86_64-unknown-none`. It follows the register contract of Microsoft's MS-DOS 4.0 for INT 21h AH=30h. The rest of the v4.0 tree is not translated. `DESIGN.md` is the module map.

## Build

The toolchain file pins `nightly-2026-11-08`.

```bash
cargo +nightly-2026-11-08 build -Zjson-target-spec --target ./i686-unknown-none.json --release
cargo +nightly-2026-11-08 build --target x86_64-unknown-none --release
```

The boot path still stops in `unimplemented!`. A later change fills the gate, the serial line, and the QEMU smoke script.
