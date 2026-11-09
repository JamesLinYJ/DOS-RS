# Missing command interpreter

After `LOADOK`, SYSINIT takes the `COMERR` path. There is no `COMMAND.COM` to exec. `BADFIL` prints the no-system-command line through INT 21h. The serial line contains `Bad or missing Command Interpreter`.

## Sub-features

- `sysinit-comerr-i686` prints `Bad or missing Command Interpreter` on i686.
- `sysinit-comerr-x86_64` prints `Bad or missing Command Interpreter` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The line follows `LOADOK` for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Missing interpreter.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `Bad or missing Command Interpreter`. The script's process status is 0.
- **Proof.** The proof transcripts contain that sentence after `LOADOK`.

## Gotchas

- `MSbio.cl3` is not in the pin. The three strings are the `MSBIO.SKL` defs this path prints, and no other message.
- The original `COMERR` jumps to `STALL`. This boot returns after the line so QEMU can still exit 33.
- `DoNotShowNum` is already 1 after CONFIG.SYS, so no line number follows the sentence.
