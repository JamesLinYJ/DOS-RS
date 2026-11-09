# Write to AUX

After the console line, the kernel writes the bytes of `AUX` and a CR LF through the auxiliary driver. Each byte is INT 14h with AH=1. That send writes the byte to COM1 and leaves the timeout bit clear.

## Sub-features

- `aux-write-i686` prints `AUX` on its own line after `BIOS` on i686.
- `aux-write-x86_64` prints `AUX` on its own line after `BIOS` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The AUX line is the fourth serial line for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Write AUX.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `AUX` after `BIOS`. The script's process status is 0.
- **Proof.** `.cursor/skills/verify-dos4-rs/proof/i686.txt` and `proof/x86_64.txt` contain the banner, the version line, `BIOS`, and `AUX`, in that order.

## Gotchas

- `AUX` is not proof if `BIOS` is missing above it. The order shows the auxiliary write ran after the console write.
- QEMU still exits 33. The script maps that to process status 0.
