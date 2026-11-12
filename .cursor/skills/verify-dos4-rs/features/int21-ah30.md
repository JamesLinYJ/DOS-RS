# Read the DOS version

INT 21h AH=30h returns the MS-DOS 4.0 version in the low 16 bits of AX, BX, and CX. The input carry bit survives each call. Two native calls exercise both input states. The guest COM probe checks the same register and flag contract.

## Sub-features

- `ah30-version` prints `AX=0004 BX=FF00 CX=0000`.
- `ah30-carry` prints `CF=1` for the first call and `CF=0` for the second.

## How to get to it (user POV)

- Run `scripts/qemu-smoke.sh`. The version results are the second and third serial lines for each target.
- There is no keyboard entry and no second command. The kernel issues the interrupt itself.

## Driving it with qemu-smoke

Preconditions:

- `scripts/qemu-smoke.sh doctor` prints `doctor ok`.
- Both release kernels exist from this run's build.

- **Read the version.** Run `PROOF_DIR=.cursor/skills/verify-dos4-rs/proof scripts/qemu-smoke.sh`. Each target prints `AX=0004 BX=FF00 CX=0000 CF=1` and `AX=0004 BX=FF00 CX=0000 CF=0`. The interpreted probe prints `GUEST ARCH OK` after its child returns. The script's process status is 0.
- **Confirm the device status.** Open `.cursor/skills/verify-dos4-rs/proof/i686.qemu-exit` and `proof/x86_64.qemu-exit`. Each file contains `33`.
- **Proof.** The `.txt` transcripts contain the banner line and the version line, in that order.

## Gotchas

- Exit 33 from QEMU is the successful debug-device status. The script turns that into process status 0. A raw `qemu-system-*` command that exits 33 has not failed.
- Both input carry states must produce their matching result.
- The version line is not proof if the banner line is missing. The banner shows which target produced the numbers.
- Do not delete the proof files while removing temp stdout. Cleanup that removes `proof/` fails the skill.
