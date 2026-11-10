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

The script passes `-display none -serial stdio -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=0x04 -m 64`, attaches `disks/msload.img` as a raw floppy, and uses a 15 second `timeout`. Read the feature file before driving. The stable strings are `dos4-rs i686`, `dos4-rs x86_64`, `AX=0004 BX=FF00 CX=0000 CF=1`, `BIOS`, `AUX`, `DSK8102`, `DSKF55AA`, `MED0001`, `CLK00000000`, `CLKP000000012400`, `CLKW0001000D`, `PRN`, `PRNS`, `PRNR0050`, `LOADOK`, `LOAD0200`, `PRNT0001`, `SYN0030`, `PAR0002`, `BUF0002`, `Unrecognized command in CONFIG.SYS`, `CMDOK`, `COMSPEC=\COMMAND.COM`, `A>`, `Microsoft DOS             Version 4.0`, `ECHO is on`, `hello`, `ECHO is off`, `BREAK is off`, `BREAK is on`, `VERIFY is off`, `VERIFY is on`, `No Path`, `PATH=\DOS`, `CLS`, `FOO=BAR`, `Current date is Tue 01-01-1980`, `Current date is Sun 03-04-1984`, `Current time is  00:00:00.00`, `Current time is  01:02:03.04`, `Volume in drive A has no label`, `Directory of  A:\\`, `COMMAND  COM`, a second `\COMMAND.COM` from TYPE, and `File not found`, `ATTR A COMMAND.COM`, `ATTR AR COMMAND.COM`, `01-01-80 00:00`, `03-04-84 01:02`, `FCB FOO.COM 0010 SR00 NXFF AL03`, `FAT12 0004 EOF REL EOF FAT16 0003 EOF REL EOF`, `SEEK 00000001 00000010 CL00 CL06 AR21`, `FATMED NC CHG CDSFFFF DTY ERR FST`, `DPB 0200 003D F8 BUF F8FF EV11 HIT DIR IO SC`, `IOCTL H06 REM00`, `PATH E5 LEN05 DR0F`, `EXEC FN01 NF02 COM`, `ALLOC 0001 0006 FIT0001 ER08 BX0006`, and `ERR 000F CL08 AC03 LC02, `IOCTL H06 REM00 FIX01`, `PATHLEN 4 OK 67 BAD`, and `EXEC LD 0100 SP00FE`, `CRIT 8001 8101 8002 8102`, `ABORT H06`, `I24 AREA02 ER0013 CL0B AC07 LC02 IG00 RT01 FL03 AB`, `DEV HS8107 L16 C04 W08 EOF1A Z`, `DINFO DX003A BX003C CX0200 AL01 AHF8`, `DIR2 CONC3 NULC4 ND VOLND MAT01 RT00 SR0000FFFF ENT40`, `DIRCALL MK00 CX0002 RM00 DV05 RT05 EX05 DOT202E2E2E`, `CREATE NW20 TR00 EX50 RO05 AT05 DA05 DIR05 EM02`, `DELETE NF02 DR05 RO05 E500`, `DISK2 BD019C 0001 004C EX0000 0000 0200 LK0021`, `DISK3 DIV0004 0000 0000 SHR0002 EOF0026 CL01 AC04 FULL0027 HIT`, `FINFO AT21 TM0841 DT0864 SZ0010 DIR10 DV02 BAD05 CL07 AC04 LC01 SET21 FO80`, `HIGHSW MSFFFF IBM0000 HIFFFF ESC1B CX18 PRN0000 EXTFFFF`, `ISEARCH DR01 AT21 E5E5 Q01 NF12 NX12 PN03 DVFFFF`, `XA G0 C0002 G1 OK G4 C0002 L0000 N0 C0002 S OK`, `IFS IN01 OUT00 ST0180 RT0003 ERCF EQ A:\\DOS`, `MSIOCTL IBMFFFF GET00 SET01 GEN0D MED66 SEC3F`, `STDIOCTL IBM0000 GET00 SET01 GEN0D MED66 SEC3F``. A successful load must not print `Bad or missing Command Interpreter`.

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
