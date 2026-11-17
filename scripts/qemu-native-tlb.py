#!/usr/bin/env python3
"""Retire a ring-3 CR3 while its secondary CPU is executing an independent ELF."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

ACK = b"TLB: AP1 ring3 handshake, vector F1, host CR3 restored, stale root rejected, page unmapped"


def check(arch, ram, proof):
    kernel = qemu.build(arch, qemu.ROOT / "target", features="tlb-selftest")
    mode = "bios" if arch == "i686" else "uefi"
    with tempfile.TemporaryDirectory(dir=ram) as temp:
        work = Path(temp)
        disk = work / "c.img"
        with disk.open("wb") as stream:
            stream.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        binary = work / "tlb.elf"
        subprocess.run([sys.executable, qemu.ROOT / "scripts/native-cc.py",
                        "--arch", arch, "-o", binary, qemu.ROOT / "native-exec/examples/tlb.c"],
                       check=True)
        subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
        args = qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
        log = proof / (arch + ".log")
        shell = Shell(args + ["-accel", "tcg,thread=multi", "-drive",
                              f"if=ide,format=raw,file={disk}"], log, work)
        try:
            shell.wait(ACK)
            shell.wait(b"A:\\> ")
            shell.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
        finally:
            shell.close()
        print(f"{arch}/{mode}: live F1 and post-retirement DOS dispatch verified; {log}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=qemu.ROOT / "target/tlb-proof")
    options = parser.parse_args()
    options.proof.mkdir(parents=True, exist_ok=True)
    for arch in options.arch or list(qemu.ARCH):
        check(arch, options.ram_dir.resolve(strict=True), options.proof)
