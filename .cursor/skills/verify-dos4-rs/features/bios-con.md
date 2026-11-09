# Write to the console

After the version line, the kernel writes the bytes of `BIOS` and a CR LF through the console driver. `STRATEGY` saves the request, then `CON$IN` indexes command 8 and calls `CON$WRIT`. Each byte is INT 29h, which is `OUTCHR`. `OUTCHR` executes INT 10h with AH=`0Eh`. That teletype writes the byte to COM1.

## Sub-features

- `con-write-i686` prints `BIOS` on its own line after the i686 version line.
- `con-write-x86_64` prints `BIOS` on its own line after the x86_64 version line.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The console line is the third serial line for each target.
- There is no keyboard. The kernel issues the write itself.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Write BIOS.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `BIOS` after `AX=0004 BX=FF00 CX=0000 CF=1`. The script's process status is 0.
- **Proof.** `.cursor/skills/verify-dos4-rs/proof/i686.txt` and `proof/x86_64.txt` each contain the banner, the version line, and `BIOS`, in that order.

## Gotchas

- `BIOS` without the version line above it is not this feature. The version line shows the console write ran after INT 21h.
- QEMU still exits 33. The script maps that to process status 0.
- A keyboard read is not part of this drive. INT 16h has no ROM keyboard and returns no key.
