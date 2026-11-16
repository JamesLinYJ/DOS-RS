#!/usr/bin/env python3
"""Compile a freestanding static ELF for the DOS-RS native process ABI."""
import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
LIBC = ROOT / "native-exec/libc"


def build(arch, sources, destination):
    compiler = arch + "-elf-gcc"
    package = "user32-cpu" if arch == "i686" else "user64-cpu"
    cpu = ROOT / "target" / package
    flags = ["-std=c11", "-O2", "-ffreestanding", "-fno-builtin", "-fno-pie",
             "-fno-stack-protector", "-fno-asynchronous-unwind-tables", "-I", LIBC / "include"]
    if arch == "x86_64":
        flags.append("-mno-red-zone")
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp)
        upstream = (cpu / "usys.S").read_text()
        template = upstream[upstream.index("#define SYSCALL"):upstream.index("SYSCALL(fork)")]
        calls = ['#include "traps.h"\n', template]
        for row in (LIBC / "syscalls.def").read_text().splitlines():
            name, number, _ = row.split()
            calls.append(f"#define SYS_sys_{name} {number}\nSYSCALL(sys_{name})\n")
        stubs = directory / "calls.S"
        stubs.write_text("".join(calls))
        subprocess.run([compiler, "-I", cpu, "-c", stubs, "-o", directory / "calls.o"], check=True)
        for name in ("unistd", "crt"):
            subprocess.run([compiler, *flags, "-c", LIBC / (name + ".c"), "-o",
                            directory / (name + ".o")], check=True)
        library = directory / "libc.a"
        subprocess.run([arch + "-elf-ar", "rcs", library, directory / "unistd.o",
                        directory / "calls.o"], check=True)
        subprocess.run([compiler, *flags, "-nostdlib", "-Wl,-T," + str(LIBC / "user.ld"),
                        "-Wl,-z,max-page-size=4096", directory / "crt.o", *sources,
                        library, "-lgcc", "-o", destination], check=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", required=True, choices=["i686", "x86_64"])
    cli.add_argument("-o", required=True, type=Path)
    cli.add_argument("sources", nargs="+", type=Path)
    args = cli.parse_args()
    build(args.arch, args.sources, args.o)
