# Working on DOS-RS

Start with the execution path you are changing. Native INT 21h calls enter through the CPU gate. Interpreted programs reach DOS through the guest run loop. Both kernel targets use the same guest implementation.

## Where to make changes

| If the change concerns… | Start here |
| --- | --- |
| CPU entry, interrupt frames, PIC or IDT | [Architecture layer](src/arch.rs) and [native INT 21h dispatch](src/int21.rs) |
| Instruction decoding, flags or segment arithmetic | [Instruction step](src/guest/step.rs), [CPU state](src/guest/state.rs) and [guest RAM](src/guest/memory.rs) |
| PSP layout, COM/MZ loading, memory ownership or EXEC | [Process loader](src/guest/process.rs), [MZ parser](src/guest/mz.rs), [paragraph arena](src/guest/arena.rs) and [run loop](src/guest/run.rs) |
| File operations, guest BIOS sector reads or shell commands | [Mounted FAT volume](src/guest/disk.rs) and [guest command executor](src/guest/command.rs) |
| An upstream routine or a boot demonstration | [BIOS](src/bios.rs), [DOS](src/dos.rs), [commands](src/cmd.rs) and [boot workloads](src/smoke.rs) |

## Contracts that cross modules

- **Registers.** Native handlers write DOS results into the low register words. For AH=30h, both adapters use the version words in `src/dos/getset.rs` and preserve incoming carry. Exercise CF clear and set when changing this path.
- **Memory.** Instruction fetch and data access share guest RAM, including segment aliases. The arena tracks paragraph ownership. Root programs, shell launches and EXEC use the same loader. A failed load must leave parent memory and available blocks intact.
- **Disk.** Guest file services, executable lookup and BIOS sector reads see the mounted volume. A file created and closed by a DOS program must be visible to its next EXEC. Failed replacement on a full volume must preserve the old file.

## Run the checks

For a quick iteration on the allocator, loader or FAT, run the host tests directly from the repository root.

```bash
mkdir -p target
rustc --edition=2024 --test tests/layout.rs -o target/layout-tests
target/layout-tests
```

For a runtime change, build both release kernels and run the [Quick start](README.md#quick-start). The smoke script also runs the host tests. After the builds, `scripts/qemu-smoke.sh doctor` checks the toolchain, both emulators, `timeout` and the kernel paths. Use the named failure and the serial output to find the first workload that diverged. Successful runs can save transcripts under `target/proof/` with `PROOF_DIR`.

The default kernel boots the virtual-terminal scheduler. Keep guest state per session, and return to the scheduler for terminal input or nested EXEC. Run `python3 scripts/qemu-terminals.py` for scheduler or input changes.

For device or port changes, also run `python3 scripts/qemu-devices.py`. The guest driver loader and interpreter forbid unsafe Rust. Keep physical I/O in the architecture and BIOS modules.

## Fixtures and translation records

- [architecture.asm](tests/architecture.asm) is the source for the COM embedded by the regression runner. After editing it, run `nasm -f bin tests/architecture.asm -o tests/architecture.com`, rebuild the kernels and run the smoke suite.
- [msload.img](disks/msload.img) exercises the native boot routines. [guest12.img](disks/guest12.img) seeds the guest FAT12 volume. Both are build inputs.
- For a translation, update the affected upstream row in the [checklist](checklist/v4.0.tsv). The [generator](scripts/gen-checklist.py) initializes rows as untranslated. Merge path changes into the reviewed inventory to preserve existing classifications.
- Record a new guest workload with its input and observable result in the [program notes](docs/guest-programs.md). Add its matching check to the [smoke harness](scripts/qemu-smoke.sh).
