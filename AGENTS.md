# Working on DOS-RS

Start from the boundary that owns the state you need to change. Both architectures use the same guest, scheduler, filesystem and device services.

| Owner | Code | Contract |
| --- | --- | --- |
| Boot | [UEFI entry](src/boot.rs), [BIOS loader](boot/bios/src/bin/loader.rs), [legacy handoff](src/boot/legacy.rs) | Validate memory ranges, stage the kernel, finish firmware calls and publish topology. The PE used by UEFI also exports the BIOS entry. |
| CPU and bus | [Architecture](src/arch.rs), [platform](src/platform.rs), [PCI](src/platform/pci.rs) | Own descriptors, AP initialization, hardware ports, PCI resource claiming, MMIO and DMA lifetimes. Keep physical access here or in the display backend. |
| Native drivers | [PCI registry](src/drivers.rs), [USB enumeration](src/drivers/usb.rs) | Register a PCI match and attach function. Implement USB class protocols against UsbIo. Keep xHCI descriptors, endpoint recovery and rings in the controller implementation. |
| Execution | [Scheduler](src/guest/run/runtime.rs), [interpreter](src/guest/step.rs) | Only the assigned thread executes its slice. Return to the boot CPU for events, system calls and I/O. A waiting or faulted guest leaves other terminals runnable. |
| Guest memory | [Address space](src/guest/memory.rs), [XMS](src/guest/run/xms.rs), [EMS](src/guest/run/ems.rs) | Keep A20, handle state and page frames per session. XMS and EMS charge a single allocator. Validate maps before publishing them. Every access through an EMS alias must see the same bytes. |
| Processes | [Loader](src/guest/process.rs), [DOS services](src/guest/run.rs), [handles](src/guest/run/handles.rs) | Publish MCBs and each PSP's JFT. DUP and inheritance share the open description and seek position. Release the child's references on return. |
| Storage | [Boot volume](src/guest/disk.rs), [DOS drivers](src/guest/run/drivers.rs) | Open, EXEC and the guest BIOS see consistent media. Preserve the previous file on allocation failure. Keep sector devices separate from DOS path handling. |
| Display | [Terminals](src/bios/pc.rs), [renderer](src/bios/screen.rs), [guest BIOS](src/guest/run/bios.rs) | A terminal owns its cells, attribute, cursor and key queue. Render the foreground terminal. Preserve DOS-supplied character attributes. |

## Checking a change

Run `python3 scripts/prepare-cpu.py` before the first build. It verifies the upstream sources against the pinned manifests. The project-owned boot, kernel, scheduler and drivers are Rust. Keep compiler-generated instructions and pinned architecture-library sources distinct from project-owned files. Guest test programs can use NASM.

- Use [qemu.py](scripts/qemu.py) to boot the interactive kernel. Test BIOS and UEFI with each supported architecture. Use [qemu-native-gate.py](scripts/qemu-native-gate.py) to test the actual native INT 21h gate with both carry inputs.
- Use [qemu-smoke.sh](scripts/qemu-smoke.sh) for host layout checks and DOS program regressions. Each emulator must exit with status 33 and print `GUEST ARCH OK`. Record output with `PROOF_DIR`.
- Use [qemu-terminals.py](scripts/qemu-terminals.py) for background execution, multiple real CPU workers, keyboard blocking, fault isolation, private EMS frames, shared files and color screenshots. Set `--firmware bios` for the legacy path.
- Use [qemu-memory.py](scripts/qemu-memory.py) for XMS/EMS operations and add `--mem /path/to/MEM.EXE` for the FreeDOS client. Exercise both the default and `--extended-mib 128` to cover pool exhaustion and classic 16-bit query saturation. Use [qemu-devices.py](scripts/qemu-devices.py) for DOS drivers and PC peripherals. Use [qemu-hardware.py](scripts/qemu-hardware.py) for high-speed USB sector reads and Ethernet RX/TX.
- Windows 3.2 acceptance requires an installation on a writable DOS-RS volume, a cold boot and a working Chinese Program Manager. Capture the actual screen. Keep original Microsoft installation media and personal disk images outside the repository.

The existing [translation inventory](checklist/v4.0.tsv) records upstream provenance. New DOS workloads belong in the [program notes](docs/guest-programs.md) with their inputs and observable results.
