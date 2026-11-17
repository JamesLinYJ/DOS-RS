#!/usr/bin/env python3
"""Exercise native fork, exec, blocking wait, groups and FD inheritance on stopped RAM disks."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


def verify(arch, root, proof):
    mode = "bios" if arch == "i686" else "uefi"
    kernel = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    disk = root / "c.img"
    with disk.open("wb") as f:
        f.truncate(4 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    for name in ("family","signal"):
        app = root / (name + ".elf")
        subprocess.run([sys.executable, qemu.ROOT / "scripts/native-cc.py", "--arch", arch,
                        "-o", app, qemu.ROOT / "native-exec/examples" / (name+".c")], check=True)
        subprocess.run(["mcopy", "-i", disk, app, "::"], check=True)
    serial = proof / (arch + ".serial")
    vm = Shell(qemu.boot_args(kernel, arch, root / "boot", 4, mode, esp_mib=36)
               + ["-drive", f"if=ide,format=raw,file={disk}"], serial, root)
    try:
        vm.wait(b"A:\\> ")
        vm.command("c:")
        vm.command("family.elf", expect=b"fork exec wait OK")
        vm.command("signal.elf", expect=b"signal delivery stop continue wait OK")
    finally:
        vm.close()
    if b"path=family.elf" not in serial.read_bytes():
        raise ValueError("missing execve trace")
    data = subprocess.check_output(["mcopy", "-i", disk, "::family.out", "-"])
    assert data == b"parent-exec-done", data
    (proof / (arch + ".out")).write_bytes(data)
    subprocess.run(["fsck.fat", "-n", disk], check=True)
    print(f"{arch}/{mode}: fork, private memory, execve, waitpid and shared file offset passed", flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", action="append", choices=qemu.ARCH)
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/native-process-proof")
    args = cli.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or qemu.ARCH:
        with tempfile.TemporaryDirectory(dir=args.ram_dir) as work:
            verify(arch, Path(work), args.proof)
