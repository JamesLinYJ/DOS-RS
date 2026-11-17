#!/usr/bin/env python3
"""Run separately compiled C processes on both native kernels and verify the stopped FAT disk."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

EXAMPLES = ("hello", "cat", "wc", "fdcheck", "private", "divide", "spin", "large")
PAYLOAD = bytes(range(256)) * 83 + b"DOS-RS"


def check(arch, firmware, work, proof):
    kernel = qemu.build(arch, qemu.ROOT / "target")
    disk = work / "c.img"
    with disk.open("wb") as f:
        f.truncate(4 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    high_images = {"highva": "0x7f0000001000", "highlow": "0x100000000"} if arch == "x86_64" else {}
    for name in (*EXAMPLES, *high_images):
        binary = work / (name + ".elf")
        source_name = "highva" if name in high_images else name
        source = qemu.ROOT / "native-exec/examples" / (source_name + ".c")
        bases = ["--text-base", high_images[name], "--data-base", "0x7ffffffff000"] if name in high_images else []
        subprocess.run([sys.executable, qemu.ROOT / "scripts/native-cc.py", "--arch", arch, *bases,
                        "-o", binary, source], check=True)
        subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
    subprocess.run(["mcopy", "-i", disk, "-", "::a.txt"], input=PAYLOAD, check=True)

    subprocess.run(["mcopy", "-i", disk, "-", "::keep.txt"], input=b"original", check=True)

    log = proof / (arch + ".log")
    shell = Shell(qemu.boot_args(kernel, arch, work / "boot", 2, firmware, esp_mib=36)
                  + ["-drive", f"if=ide,format=raw,file={disk}"], log, work)
    try:
        shell.wait(b"A:\\> ")
        shell.command("c:")
        shell.command("hello.elf 7", expect=b"hello from native C")
        shell.command("cat.elf a.txt | wc.elf -c > count.txt")
        shell.command("echo $?", expect=b"\r\n0\r\n")
        shell.command("type count.txt", expect=str(len(PAYLOAD)).encode() + b"\n")
        shell.command("cat.elf < a.txt | cat.elf > copy.bin")
        shell.command("fdcheck.elf", expect=b"unlink, shared offset, pipe EOF OK")
        shell.command("private.elf send | private.elf receive", expect=b"private BSS and distinct PIDs OK")
        if arch == "x86_64":
            shell.command("highva.elf send | highva.elf receive", expect=b"high VA exec, private data and syscall buffers OK")
            for mode in ("write", "execute"):
                shell.command("highva.elf " + mode, expect=b"user exception 0e")
            shell.command("highlow.elf send | highva.elf receive", expect=b"high VA exec, private data and syscall buffers OK")
            shell.command("highlow.elf hole", expect=b"user exception 0e")
            shell.command("hello.elf 7", expect=b"hello from native C")
        shell.command("large.elf | large.elf", expect=b"large image loaded")
        # Failure to load a later stage must leave the original redirection target intact.
        shell.command("large.elf | missing.elf > keep.txt", expect=b"no such file or directory")
        shell.command("divide.elf", expect=b"user exception 00")
        shell.command("echo $?", expect=b"\r\n128\r\n")
        start = log.stat().st_size
        shell.enter("cat.elf | wc.elf -c > keys.txt")
        shell.key("alt-f2")
        shell.wait(b"DOS terminal 2", start)
        shell.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
        shell.key("alt-f1")
        start = log.stat().st_size
        shell.enter("hello")
        shell.key("ctrl-d")
        shell.wait(b"C:\\> ", start)
        start = log.stat().st_size
        shell.enter("spin.elf")
        shell.wait(b"CPU loop started", start)
        shell.key("alt-f2")
        shell.command("c:")
        shell.command("fdcheck.elf", expect=b"unlink, shared offset, pipe EOF OK")
    finally:
        shell.close()
    for name, expected in [("count.txt", f"{len(PAYLOAD)}\n".encode()),
                           ("copy.bin", PAYLOAD), ("keys.txt", b"6\n"), ("keep.txt", b"original")]:
        actual = subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])
        if actual != expected:
            raise RuntimeError(f"{arch}: incorrect bytes in {name}")
    print(f"{arch}/{firmware}: counted and copied {len(PAYLOAD)} bytes; unlink, isolation, "
          "fault handling, timer preemption and DOS coexistence passed", flush=True)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--ram-dir", type=Path, required=True)
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/native-elf-proof")
    cli.add_argument("--arch", choices=["i686", "x86_64"], action="append")
    args = cli.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or ["i686", "x86_64"]:
        with tempfile.TemporaryDirectory(dir=args.ram_dir.resolve()) as work:
            check(arch, "bios" if arch == "i686" else "uefi", Path(work), args.proof)


if __name__ == "__main__":
    main()
