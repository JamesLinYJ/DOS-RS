#!/usr/bin/env python3
"""Build upstream musl and run its static ELF on both DOS-RS schedulers."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile
from build import build, ROOT, PORT
from cc import compile

sys.path.insert(0, str(ROOT / "scripts"))
import qemu
from qemu_shell import Shell

PASSED = b"musl static TLS pthreads condvar malloc stdio fork exec wait sleep signal OK"


def check(arch, archive, ram, proof):
    sysroot = build(arch, archive, ROOT / "target/musl" / arch)
    kernel = qemu.build(arch, ROOT / "target")
    with tempfile.TemporaryDirectory(dir=ram) as temporary:
        work = Path(temporary)
        binary = work / "libtest.elf"
        compile(arch, [PORT / "tests/libc.c"], binary)
        headers = subprocess.check_output([arch + "-elf-readelf", "-lW", binary])
        if b" TLS " not in headers or b"INTERP" in headers:
            raise RuntimeError("missing static ELF TLS program header")
        (proof / (arch + "-elf.txt")).write_bytes(headers)
        disk = work / "c.img"
        with disk.open("wb") as f:
            f.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
        mode = "bios" if arch == "i686" else "uefi"
        launch = qemu.boot_args(kernel, arch, work / "boot", 4, mode, esp_mib=36)
        launch += ["-accel", "tcg,thread=multi", "-drive", "if=ide,format=raw,file=" + str(disk)]
        shell = Shell(launch, proof / (arch + ".log"), work)
        try:
            shell.wait(b"A:\\> ")
            shell.command("c:")
            shell.command("libtest.elf probe")
            shell.command("echo $?")
            report = shell.log.read_bytes()
            if PASSED not in report or b"echo $?\r\n0\r\n" not in report:
                raise RuntimeError("libc assertions failed: " + repr(report[-500:]))
        finally:
            shell.close()
        expected = b"argc=2 sum=21 tls=37\n"
        data = subprocess.check_output(["mcopy", "-i", disk, "::musl.txt", "-"])
        if data != expected:
            raise RuntimeError("stopped disk differs: " + repr(data))
        print(f"{arch}/{mode} musl-1.2.6: {sysroot}; {len(data)} stdio bytes matched")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=ROOT / "target/musl-proof")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or list(qemu.ARCH):
        check(arch, args.archive.resolve(), args.ram_dir.resolve(strict=True), args.proof)
