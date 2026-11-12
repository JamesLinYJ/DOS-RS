# DOS-RS

DOS-RS is a freestanding Rust port of selected MS-DOS 4.0 services. It boots an i686 or x86_64 kernel under QEMU. Both kernels run the same real-mode interpreter, load COM and MZ executables from a RAM-backed FAT12 volume, and exercise DOS programs including MASM, LINK and the Microsoft C compiler passes.

The current boot entry runs a fixed regression sequence. The shell reads the bundled batch workload. Serial output includes the native interrupt results, guest program output and generated-file checks. Boot finishes through QEMU's debug-exit device.

- [Architecture and implementation boundaries](DESIGN.md)
- [Guest programs and expected results](docs/guest-programs.md)
- [Upstream translation checklist](checklist/v4.0.tsv) and [source attribution](NOTICE)

## Build and verify

Install Rust through rustup, both QEMU system emulators, Bash and a `timeout` executable. The toolchain file selects `nightly-2026-11-08` with `rust-src`. Run from the repository root.

```bash
cargo build -Zjson-target-spec --target ./i686-unknown-none.json --release
cargo build --target x86_64-unknown-none --release
scripts/qemu-smoke.sh doctor
PROOF_DIR=target/proof scripts/qemu-smoke.sh
```

The last command runs the host layout tests and checks the full serial transcript from each kernel. Both native calls return `AX=0004 BX=FF00 CX=0000`, with `CF=1` and `CF=0` respectively. The interpreted regression prints `GUEST ARCH OK`. Each QEMU process returns 33 after the kernel writes `0x10` to port `0xF4`. The script returns zero when every check passes.

The checked-in guest regression binary builds from [8086 assembly](tests/architecture.asm). To verify a source change with NASM, rebuild it before building the kernels.

```bash
nasm -f bin tests/architecture.asm -o tests/architecture.com
```

## Translation status

The checklist tracks 1,280 paths under upstream `v4.0/src`. It currently labels 11 translated, 154 partial and 1,115 untranslated. These are source-file classifications. Runtime coverage is recorded separately by the smoke suite. `scripts/gen-checklist.py` generates an initial path inventory with all rows marked untranslated. Preserve reviewed statuses when updating an existing checklist.
