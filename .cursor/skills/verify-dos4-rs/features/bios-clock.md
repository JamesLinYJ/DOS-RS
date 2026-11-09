# Read the clock

After the disk line, the kernel reads the clock with INT 1Ah AH=0 and tick count CX=0 DX=0. The gate returns that count and clears AL. `TIM$READ` stores day 0 and midnight. The serial line shows `CLK00000000`. A second read enters INT 1Ah with CX=1 and DX=0x0E. Those 65550 ticks become 1:00:00.36, and the six clock bytes print as `CLKP000000012400`. `TIM$WRIT` of that same record runs `TIME_TO_TICKS` and prints `CLKW0001000D`.

## Sub-features

- `clock-read-i686` prints `CLK00000000` after `DSK8102` on i686.
- `clock-read-x86_64` prints `CLK00000000` after `DSK8102` on x86_64.
- `clock-force-i686` prints `CLKP000000012400` on i686.
- `clock-force-x86_64` prints `CLKP000000012400` on x86_64.
- `clock-write-i686` prints `CLKW0001000D` on i686.
- `clock-write-x86_64` prints `CLKW0001000D` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The clock line is the sixth serial line for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Read the clock.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `CLK00000000`, then `CLKP000000012400`, then `CLKW0001000D`. The script's process status is 0.
- **Proof.** The proof transcripts contain `CLK00000000` after `DSK8102`, and `CLKP000000012400` after that.

## Gotchas

- `CLK00000000` means day 0 and hour and minute 0. It is not a wall-clock time from a CMOS chip. `HaveCMOSClock` is 0.
- `CLKP000000012400` is the six stored bytes for CX=1 DX=0x0E, in memory order. Minutes are byte 2 and hours are byte 3, so the hour is the `01` in the middle of that line.
- QEMU still exits 33. The script maps that to process status 0.
