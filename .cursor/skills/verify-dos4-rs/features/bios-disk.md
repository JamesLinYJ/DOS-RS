# Read one disk sector

After the AUX line, the kernel reads one sector from logical drive 0. INT 13h returns timeout AH=`80h` because QEMU `-kernel` has no disk controller. `MAPERROR` turns `80h` into AL=2. The status word is `8102h`. The serial line shows `DSK8102`.

## Sub-features

- `disk-read-i686` prints `DSK8102` after `AUX` on i686.
- `disk-read-x86_64` prints `DSK8102` after `AUX` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The disk line is the fifth serial line for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Read sector 0.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `DSK8102`. The script's process status is 0.
- **Proof.** The proof transcripts contain `DSK8102` after `AUX`.

## Gotchas

- `DSK8102` is the error status, not a successful sector. A line that says `DSK0100` would mean INT 13h returned success, which this machine does not.
- QEMU still exits 33. The script maps that to process status 0.
