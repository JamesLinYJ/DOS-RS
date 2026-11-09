# Boot the kernel

Boot shows a one-line serial banner for the target that just started, then continues into the version call.

## Sub-features

- `boot-i686` prints `dos4-rs i686` on COM1.
- `boot-x86_64` prints `dos4-rs x86_64` on COM1.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh` after the release kernels are built.
- There is no interactive prompt. The banner is the first serial line.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- `target/i686-unknown-none/release/dos4-rs` and `target/x86_64-unknown-none/release/dos4-rs` are the binaries this run built.

- **Boot both targets.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Stdout contains `dos4-rs i686` and `dos4-rs x86_64`. The script's process status is 0.
- **Proof.** Open `.cursor/skills/verify-dos4-rs/proof/i686.txt` and `proof/x86_64.txt`. Each file starts with its banner line.

## Gotchas

- QEMU itself exits 33. That is the debug device, not a failed boot. The script maps 33 to process status 0.
- A missing banner with process status 0 means the script was not the thing that ran.
- `timeout` exits 124 when a kernel hangs. Do not treat 124 as the debug-device status.
