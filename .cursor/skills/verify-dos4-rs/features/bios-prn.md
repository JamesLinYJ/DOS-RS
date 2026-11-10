# Write to the printer

After the clock lines, the kernel writes the bytes of `PRN` and a CR LF through the printer driver. Each byte is INT 17h with AH=0. The handler writes the byte to COM1 and returns AH=0, so the timeout bit is clear. A status call with AH=2 returns not-busy and prints `PRNS`. The generic ioctl get-retry call prints `PRNR0050`. Output-until-busy sends one `.` and then prints `PRNT0001` with those printer lines. It is not a SYSINIT step.

## Sub-features

- `prn-write-i686` prints `PRN` on i686.
- `prn-write-x86_64` prints `PRN` on x86_64.
- `prn-stat-i686` prints `PRNS` on i686.
- `prn-stat-x86_64` prints `PRNS` on x86_64.
- `prn-retry-i686` prints `PRNR0050` on i686.
- `prn-retry-x86_64` prints `PRNR0050` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The printer line follows the clock line.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Write PRN.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `PRN`, `PRNS`, and `PRNR0050`. The script's process status is 0.
- **Proof.** The proof transcripts contain those three printer lines.

## Gotchas

- `PRN` is not proof if the clock line is missing above it.
- QEMU still exits 33. The script maps that to process status 0.
