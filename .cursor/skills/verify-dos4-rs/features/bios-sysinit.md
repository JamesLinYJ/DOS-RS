# Missing command interpreter

After `LOADOK`, SYSINIT parses the decimal number 48 with `GETNUM` and prints `SYN0030`. `ParaRound` of 17 bytes is 2 paragraphs, printed `PAR0002`. Then `COMERR` runs. There is no `COMMAND.COM` to exec. `BADFIL` prints the no-system-command line through INT 21h. The serial line contains `Bad or missing Command Interpreter`.

## Sub-features

- `sysinit-comerr-i686` prints `Bad or missing Command Interpreter` on i686.
- `sysinit-comerr-x86_64` prints `Bad or missing Command Interpreter` on x86_64.
- `sysinit-num-i686` prints `SYN0030` on i686.
- `sysinit-num-x86_64` prints `SYN0030` on x86_64.
- `sysinit-round-i686` prints `PAR0002` on i686.
- `sysinit-round-x86_64` prints `PAR0002` on x86_64.

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
