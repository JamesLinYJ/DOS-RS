#!/usr/bin/env python3
"""Exercise XMS and EMS from real-mode clients on both kernels and firmware paths."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import qemu


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--mem", type=Path, help="optional FreeDOS 1.12 MEM.EXE")
    cli.add_argument(
        "--extended-mib", type=int, default=8, help="extended MiB per DOS session"
    )
    args = cli.parse_args()
    os.environ["DOS_RS_EXT_MIB"] = str(args.extended_mib)
    root = qemu.ROOT / f"target/memory-proof-{args.extended_mib}"
    root.mkdir(parents=True, exist_ok=True)
    disk = root / "dos.img"
    shutil.copyfile(qemu.ROOT / "disks/guest12.img", disk)
    probe = root / "CHECKMEM.COM"
    subprocess.run(
        ["nasm", "-f", "bin", qemu.ROOT / "tests/extended-memory.asm", "-o", probe],
        check=True,
    )
    subprocess.run(["mcopy", "-o", "-i", disk, probe, "::CHECKMEM.COM"], check=True)
    commands = ["CHECKMEM.COM"]
    if args.mem:
        subprocess.run(["mcopy", "-o", "-i", disk, args.mem, "::MEM.EXE"], check=True)
        commands += ["MEM /EMS", "MEM /XMS"]
    batch = root / "AUTOEXEC.BAT"
    batch.write_bytes(("\r\n".join(commands) + "\r\n").encode())
    subprocess.run(["mcopy", "-o", "-i", disk, batch, "::"], check=True)
    for arch in qemu.ARCH:
        kernel = qemu.build(
            arch,
            qemu.ROOT / f"target/memory-build-{args.extended_mib}",
            disk,
            "batch-console",
        )
        for mode in ["uefi", "bios"]:
            work = root / (arch + "-" + mode)
            log = work / "serial.txt"
            cmd = qemu.boot_args(kernel, arch, work, 4, mode) + [
                "-display",
                "none",
                "-serial",
                f"file:{log}",
                "-monitor",
                "none",
                "-nic",
                "none",
                "-device",
                "isa-debug-exit,iobase=0xf4,iosize=0x04",
            ]
            result = subprocess.run(cmd, timeout=45)
            output = log.read_text(errors="replace")
            markers = ["XMS EMS MEMORY CHECK PASSED", "Batch complete."]
            if args.mem:
                markers += [
                    "EMS page frame",
                    "Free upper memory         98,288 bytes",
                    "Free EMS memory",
                    "XMS version",
                    f"Free XMS memory           {(args.extended_mib * 1024 - 64) * 1024:,} bytes",
                    f"({(args.extended_mib * 1024 - 64) * 1024:,} bytes)",
                ]
            if (
                result.returncode != 33
                or "Exit code: 255" in output
                or not all(s in output for s in markers)
            ):
                raise RuntimeError(f"{arch}/{mode}: {output}")
            print(f"{arch}/{mode} passed ({log})", flush=True)


if __name__ == "__main__":
    main()
