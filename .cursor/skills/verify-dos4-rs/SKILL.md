---
name: verify-dos4-rs
description: Drive the dos4-rs QEMU kernels on i686 and x86_64 and prove the boot banner and INT 21h AH=30h. Use when changing the boot gate, the serial line, or the version registers.
---

# Verify dos4-rs

The user-facing surface is a pair of QEMU kernels. There is no browser and no long-running server. A run builds the binaries, then boots each one. Two runs can go at once. They do not bind a port. Give each run its own `PROOF_DIR` so the transcripts do not overwrite each other.

## Launch

From the repo root, with the toolchain pinned by `rust-toolchain.toml`:

```bash
cargo +nightly-2026-11-08 build -Zjson-target-spec --target ./i686-unknown-none.json --release
cargo +nightly-2026-11-08 build --target x86_64-unknown-none --release
scripts/qemu-smoke.sh doctor
```

Ready means `doctor ok` on stdout and both release binaries exist. A build that only typechecks the host is not ready.

There is nothing to keep alive after a boot. Each drive starts QEMU, waits for it to exit, and removes the temp file it created.

## Doctor

```bash
scripts/qemu-smoke.sh doctor
```

The command checks `qemu-system-i386`, `qemu-system-x86_64`, `timeout`, rustc `1.102.0-nightly` from `nightly-2026-11-08`, and both release kernels. Exit 0 and the line `doctor ok` means the tree is worth driving. Anything else means do not boot.

## Drive

The harness is `scripts/qemu-smoke.sh`. It boots `target/i686-unknown-none/release/dos4-rs` with `qemu-system-i386` and `target/x86_64-unknown-none/release/dos4-rs` with `qemu-system-x86_64`.

```bash
PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh
```

The script passes `-display none -serial stdio -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=0x04 -m 64`, attaches `disks/msload.img` as a raw floppy, and uses a 15 second `timeout`. Read the feature file before driving. The stable strings are `dos4-rs i686`, `dos4-rs x86_64`, `AX=0004 BX=FF00 CX=0000 CF=1`, `BIOS`, `AUX`, `DSK8102`, `DSKF55AA`, `MED0001`, `CLK00000000`, `CLKP000000012400`, `CLKW0001000D`, `PRN`, `PRNS`, `PRNR0050`, `LOADOK`, `LOAD0200`, `PRNT0001`, `SYN0030`, `PAR0002`, `BUF0002`, `Unrecognized command in CONFIG.SYS`, `CMDOK`, `COMSPEC=\COMMAND.COM`, `A>`, `Microsoft DOS             Version 4.0`, `ECHO is on`, `hello`, `ECHO is off`, `BREAK is off`, `BREAK is on`, `VERIFY is off`, `VERIFY is on`, `No Path`, `PATH=\DOS`, `CLS`, `FOO=BAR`, `Current date is Tue 01-01-1980`, `Current date is Sun 03-04-1984`, `Current time is  00:00:00.00`, `Current time is  01:02:03.04`, and `Volume in drive A has no label`. A successful load must not print `Bad or missing Command Interpreter`.

## Evidence

Write transcripts under `.cursor/skills/verify-dos4-rs/proof/`. `PROOF_DIR` is that directory. The script copies each QEMU stdout to `$PROOF_DIR/<target>.txt` and the raw QEMU status to `$PROOF_DIR/<target>.qemu-exit`.

Proof standards:

- Boot the kernel. Do not call `int21::entry` from a host test.
- Capture stdout, which is the serial line, and the QEMU status before the script maps 33 to 0.
- The result line is the side effect. There is no disk file and no network call.
- QEMU is the real machine the kernel runs on. Do not replace it with a mock of the serial port.

## Cleanup

The script deletes its temp stdout after a successful boot. It does not delete `PROOF_DIR`. Do not remove `.cursor/skills/verify-dos4-rs/proof/`.

If you start QEMU yourself and it is still running, kill that process id. Do not kill by the name `qemu-system-i386` or `qemu-system-x86_64`, because another run may own a different process. `timeout` in the script already stops a hung boot from this run.

After cleanup, `proof/i686.txt` and `proof/x86_64.txt` must still be there.

## Helpers

`scripts/qemu-smoke.sh` is the only helper. It is executable.

```bash
scripts/qemu-smoke.sh doctor
PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh
```

`scripts/gen-checklist.py` regenerates the translation checklist. It does not boot the kernel.
