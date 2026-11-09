# Read the clock

After the disk line, the kernel reads the clock with INT 1Ah AH=0. The handler returns ticks of 0 and no day rollover. `TIM$READ` stores day 0 and midnight. The serial line shows `CLK00000000`.

## Sub-features

- `clock-read-i686` prints `CLK00000000` after `DSK8102` on i686.
- `clock-read-x86_64` prints `CLK00000000` after `DSK8102` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The clock line is the sixth serial line for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Read the clock.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `CLK00000000`. The script's process status is 0.
- **Proof.** The proof transcripts contain `CLK00000000` after `DSK8102`.

## Gotchas

- `CLK00000000` means day 0 and hour and minute 0. It is not a wall-clock time from a CMOS chip. `HaveCMOSClock` is 0.
- QEMU still exits 33. The script maps that to process status 0.
