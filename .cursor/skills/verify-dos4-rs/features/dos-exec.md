# Load COMMAND.COM

After the unrecognized CONFIG line, INT 21h AH=4Bh AL=0 loads `COMMAND.COM` from the floppy image. The payload begins with `\COMMAND.COM`. The serial line shows `CMDOK`. The missing-interpreter line still follows.

## Sub-features

- `exec-command-i686` prints `CMDOK` on i686.
- `exec-command-x86_64` prints `CMDOK` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. `CMDOK` follows the CONFIG line.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Load the shell.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `CMDOK` and still prints `Bad or missing Command Interpreter`. The script's process status is 0.
- **Proof.** The proof transcripts contain `CMDOK` before that interpreter line.

## Gotchas

- AL=2 is not a legal exec function. AL=0 loads.
- The file is the second root entry, cluster 3. `IO.SYS` stays cluster 2.
