#!/usr/bin/env python3
"""Run musl socketpair/posix_spawn on both native kernels and inspect the stopped disk."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


def check(arch, args):
    mode = "bios" if arch == "i686" else "uefi"
    kernel = args.kernel or qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    proof = args.proof / arch
    proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="local-sockets-", dir=args.ram_dir) as tmp:
        work = Path(tmp)
        binary = work / "socktest.elf"
        subprocess.run([sys.executable, qemu.ROOT / "ports/musl/cc.py", "--arch", arch,
                        "-o", binary, qemu.ROOT / "ports/musl/tests/socket_spawn.c"], check=True)
        disk = work / "c.img"
        with disk.open("wb") as file:
            file.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
        command = qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
        command.extend(["-drive", f"if=ide,format=raw,file={disk}"])
        guest = Shell(command, proof / "serial.log", work)
        try:
            guest.wait(b"A:\\> ", timeout=240)
            guest.command("c:", timeout=120)
            guest.command("socktest.elf issue8", timeout=240,
                          expect=b"local sockets and posix_spawn passed")
            guest.command("echo $?", expect=b"\r\n0\r\n")
        finally:
            guest.close()
        qemu.check_native_boot(guest.log, arch, mode)
        actual = subprocess.check_output(["mcopy", "-i", disk, "::spawn.out", "-"])
        if actual != b"parent":
            raise RuntimeError(f"stopped spawn file differs: {actual!r}")
        checker = shutil.which("fsck.fat") or "/opt/homebrew/sbin/fsck.fat"
        with (proof / "fsck.log").open("wb") as log:
            subprocess.run([checker, "-n", disk], stdout=log, stderr=subprocess.STDOUT, check=True)
        (proof / "spawn.out").write_bytes(actual)
        hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest()
                  for name, path in [("kernel", kernel), ("ELF", binary), ("disk", disk),
                                     ("libc", qemu.ROOT / "target/musl" / arch / "sysroot/lib/libc.a")]}
        result = {"arch": arch, "kernel_bits": 32 if arch == "i686" else 64,
                  "firmware": mode, "sha256": hashes,
                  "spawn_bytes": len(actual),
                  "qemu": subprocess.check_output(
                      ["qemu-system-" + qemu.ARCH[arch][0], "--version"], text=True).splitlines()[0]}
        (proof / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"{arch}/{mode}: sockets, backpressure, descriptor exhaustion and posix_spawn passed",
              flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=list(qemu.ARCH), action="append")
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/local-sockets-proof")
    cli.add_argument("--kernel", type=Path, help="frozen kernel for exactly one --arch")
    args = cli.parse_args()
    if args.kernel and len(args.arch or ()) != 1:
        cli.error("--kernel requires one --arch")
    args.ram_dir = args.ram_dir.resolve(strict=True)
    for arch in args.arch or qemu.ARCH:
        check(arch, args)
