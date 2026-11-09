# dos4-rs

`dos4-rs` is a `no_std` kernel for `i686-unknown-none` and `x86_64-unknown-none`. It boots under QEMU and serves INT 21h AH=30h with the MS-DOS 4.0 register contract. `AX` is `0004`, `BX` is `FF00`, `CX` is `0000`, and carry stays set. The rest of the v4.0 tree is listed in the checklist and is not translated.

`DESIGN.md` is the module map. `NOTICE` names the upstream commit.

## Build

The toolchain file pins `nightly-2026-11-08`.

```bash
cargo +nightly-2026-11-08 build -Zjson-target-spec --target ./i686-unknown-none.json --release
cargo +nightly-2026-11-08 build --target x86_64-unknown-none --release
```

## Boot

`scripts/qemu-smoke.sh` boots both kernels. QEMU `isa-debug-exit` cannot return process status 0. The kernel writes `0x10` to port `0xF4`, QEMU exits 33, and the script maps 33 to 0.

```bash
scripts/qemu-smoke.sh
```

You should see a banner and one result line for each target.

```text
dos4-rs i686
AX=0004 BX=FF00 CX=0000 CF=1
dos4-rs x86_64
AX=0004 BX=FF00 CX=0000 CF=1
```

## Checklist

`checklist/v4.0.tsv` has one row per file under upstream `v4.0/src` at commit `2d04cacc5322951f187bb17e017c12920ac8ebe2`. Regenerate it when you have that tree and the path list:

```bash
scripts/gen-checklist.py /path/to/msdos-files.txt /path/to/upstream > checklist/v4.0.tsv
```
