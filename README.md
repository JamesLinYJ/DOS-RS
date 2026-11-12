# DOS-RS

![DOS-RS booting its i686 kernel and running DOS guest programs](docs/assets/boot.svg)

DOS-RS runs 16-bit DOS programs on two bare-metal Rust kernels. Boot either target in QEMU and the same interpreter runs COM and MZ executables against a writable FAT12 volume. The bundled workload assembles, compiles and links DOS programs, then executes the files it produced.

[Quick start](#quick-start) · [What runs](#what-runs) · [Contributor guide](AGENTS.md)

<details>
<summary>中文简介</summary>

DOS-RS 用 Rust 实现了部分 MS-DOS 4.0 服务。i686 和 x86_64 两个内核共用 16 位解释器。启动后，预设批处理会运行 MASM、LINK 和 Microsoft C 编译工具，并执行生成的 DOS 程序。下方命令可以构建两个内核、运行 QEMU 验证并保存串口记录。

</details>

## Quick start

From the repository root, use a shell with **rustup**, **Bash**, **QEMU** (`qemu-system-i386` and `qemu-system-x86_64`) and **`timeout`** on the path. Rustup reads the pinned nightly and components from [rust-toolchain.toml](rust-toolchain.toml).

```bash
cargo build -Zjson-target-spec --target ./i686-unknown-none.json --release
cargo build --target x86_64-unknown-none --release
PROOF_DIR=target/proof scripts/qemu-smoke.sh
```

The last command runs the host tests, boots each kernel and checks its serial output. A passing run prints `GUEST ARCH OK` for each target and returns **0**. The raw QEMU status is **33**, the success value from the debug-exit device. Open `target/proof/i686.txt` or `target/proof/x86_64.txt` to read a boot transcript.

On macOS, open the batch in QEMU’s serial console window. The console build uses its own output directory so the smoke commands above can reuse their release binaries.

```bash
cargo build --release --features batch-console --target x86_64-unknown-none --target-dir target/console
qemu-system-x86_64 -kernel target/console/x86_64-unknown-none/release/dos4-rs \
  -display cocoa -vga none -serial vc:88Cx22C -monitor none -nic none -m 64
```

## DOS drivers and PC hardware

Place a flat or MZ device image on the guest FAT volume and add its path to `CONFIG.SYS`. Drivers initialize before the shell runs `AUTOEXEC.BAT`. For example, to try FreeDOS SETVER on a copy of the bundled volume:

```sh
mkdir -p target
cp disks/guest12.img target/with-drivers.img
mcopy -o -i target/with-drivers.img /path/to/SETVER.SYS ::SETVER.SYS
printf 'DEVICE=A:\\SETVER.SYS\r\n' > target/CONFIG.SYS
mcopy -o -i target/with-drivers.img target/CONFIG.SYS ::CONFIG.SYS
DOS_RS_GUEST_IMAGE="$PWD/target/with-drivers.img" cargo build --release --features batch-console \
  --target x86_64-unknown-none --target-dir target/console
qemu-system-x86_64 -kernel target/console/x86_64-unknown-none/release/dos4-rs \
  -display cocoa -vga std -serial stdio -monitor none -nic none -m 64
```

The loader follows linked device headers, calls strategy and interrupt with DOS request packets, and retains the paragraphs returned by INIT. `DEVICEHIGH` uses conventional memory. Registered character devices support named handle reads, writes, and basic IOCTL. Block units start at absolute drive 2 and support 16-bit-sector INT 25h/26h requests. The FAT file API currently mounts the embedded boot volume. Drivers needing XMS/EMS, DMA, hardware IRQs or other PC controllers require more kernel support.

| Built-in device | Available through |
| --- | --- |
| VGA 80×25 text output | Kernel console and INT 10h teletype |
| PS/2 keyboard | INT 16h read and peek, DOS console input |
| 16550-compatible COM1 and COM2 | Device handles, INT 14h and UART ports |
| LPT1 parallel printer | `PRN` and `LPT1`, INT 17h and parallel ports |
| Primary IDE master | Read-only 512-byte ATA PIO through INT 13h extensions 41h and 42h on drive 80h |

With Python 3, NASM and mtools installed, run the dedicated device suite. It builds temporary guest media, boots both kernels, rejects failed INITs, reads and writes a resident character and block device, checks a sector from a separate IDE disk, injects a PS/2 key and captures independent serial and printer output.

```sh
python3 scripts/qemu-devices.py
# Also exercise the real driver and inspect its installed version-table pointer.
python3 scripts/qemu-devices.py --setver /path/to/SETVER.SYS
```

## What runs

| Workload | What to look for in the serial output |
| --- | --- |
| COM and MZ loading | `GUEST HELLO COM` and `HELLO EXE` come from two different executable formats. |
| Child processes | `FILE Z EXEC 7` shows a parent reading a file, launching a child and receiving its exit code. |
| DOS development tools | `MASM OK` comes from the assembled and linked program. `L1 OK` comes from the large-model C program. |
| Runtime regression | `GUEST ARCH OK` covers the version registers, a self-modifying instruction and nested EXEC. |

Boot follows a scripted batch session. Interactive command entry and full DOS file-handle inheritance remain open. The program notes list the commands, input files and expected results for individual tools. [Guest program notes](docs/guest-programs.md)

## Source and provenance

The translation checklist maps the Rust modules to files in Microsoft’s published MS-DOS 4.0 source tree. [Translation checklist](checklist/v4.0.tsv)

DOS-RS is MIT-licensed. The upstream revision and Microsoft copyright attribution are recorded in the repository. [License](LICENSE) · [Notice](NOTICE)
