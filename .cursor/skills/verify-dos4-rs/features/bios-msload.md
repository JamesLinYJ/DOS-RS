# Load IO.SYS

After the printer lines, the kernel reads `IO.SYS` from the floppy image through INT 13h and follows the FAT chain until the end marker. The first two bytes of that file are `OK`, and the serial line shows `LOADOK`. The loaded length is one cluster, `LOAD0200`. Drive 0x80 still times out, so `DSK8102` stays on the earlier disk line.

## Sub-features

- `msload-i686` prints `LOADOK` after `PRN` on i686.
- `msload-x86_64` prints `LOADOK` after `PRN` on x86_64.
- `msload-size-i686` prints `LOAD0200` on i686.
- `msload-size-x86_64` prints `LOAD0200` on x86_64.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The load line is the serial line after `PRN` for each target.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.
- `disks/msload.img` is attached by the script as a raw floppy.

- **Load IO.SYS.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `LOADOK` and `LOAD0200`. The script's process status is 0.
- **Proof.** The proof transcripts contain `LOADOK` after `PRN`.

## Gotchas

- The image is compiled into the kernel as well as attached with `-drive`. QEMU does not have to service the read. The INT 13h gate copies the sector.
- A missing `IO.SYS` prints the no-system line and does not print `LOADOK`. `MSbio.cl1` is not in the pin. That line is the one `SYSMSG` string.
