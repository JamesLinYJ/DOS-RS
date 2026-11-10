# dos4-rs verification map

This directory is the maintained source for verifying the user-facing behavior of the dos4-rs kernels. Read the index before driving, then use the matching feature file as the recipe.

## Baseline preconditions

- Build both release kernels with the commands in the skill's Launch section.
- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Set `PROOF_DIR` to a directory this run owns. The committed proof directory is `.cursor/skills/verify-dos4-rs/proof`.
- Do not drive a binary this run did not build.

## Driving conventions

- Start from a fresh QEMU process. The kernel does not keep state between boots.
- Treat the serial strings and the QEMU status as literal.
- Run boots through `scripts/qemu-smoke.sh`.
- Leave proof files in place during cleanup.

## Proof and skip reporting

- Capture the serial transcript and the raw QEMU status, not only the script's process status.
- Record the feature id and the target name with every artifact.
- Report an unreachable path with the command and the unmet precondition.
- Do not report a skipped target as verified through the other target.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short ids with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with qemu-smoke` starts with `Preconditions:` and pairs each action with a command and an observable result.
4. `Gotchas` lists traps that can waste or invalidate a run.

## Features

- [Boot the kernel](boot.md) covers the serial banner on both targets.
- [Read the DOS version](int21-ah30.md) covers INT 21h AH=30h, including carry.
- [Write to the console](bios-con.md) covers CON$WRIT through INT 29h and INT 10h AH=0Eh.
- [Write to AUX](bios-aux.md) covers AUX$WRIT through INT 14h AH=1.
- [Read one disk sector](bios-disk.md) covers a one-sector read that returns status `8102h`.
- [Read the clock](bios-clock.md) covers INT 1Ah AH=0, a zero tick count, and CX=1 DX=0x0E.
- [Write to the printer](bios-prn.md) covers INT 17h AH=0.
- [Load IO.SYS](bios-msload.md) covers the drive 0 read of `IO.SYS` and the no-system message path's text.
- [Missing command interpreter](bios-sysinit.md) covers `COMERR` printing through INT 21h AH=9 and AH=2.
- [Load COMMAND.COM](dos-exec.md) covers INT 21h AH=4Bh reading the shell from the floppy image.
