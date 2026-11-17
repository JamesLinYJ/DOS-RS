#!/usr/bin/env python3
"""Link a static ELF against the installed DOS-RS musl sysroot."""
import argparse
from pathlib import Path
import subprocess
from build import PORT, ROOT


def compile(arch, sources, out):
    root = ROOT / "target/musl" / arch / "sysroot"
    gcc = arch + "-elf-gcc"
    builtin = subprocess.check_output([gcc, "-print-file-name=include"], text=True).strip()
    flags = ["-mno-red-zone"] if arch == "x86_64" else []
    subprocess.run([
        gcc, "-O2", "-fno-pie", "-nostdinc", "-isystem", root / "include", "-isystem", builtin,
        *flags, "-static", "-nostdlib", "-Wl,-z,max-page-size=4096", "-Wl,-z,noexecstack",
        "-Wl,-T," + str(PORT / "user.ld"), root / "lib/crt1.o", *sources,
        "-Wl,--start-group", root / "lib/libc.a", "-lgcc", "-Wl,--end-group", "-o", out,
    ], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=["i686", "x86_64"], required=True)
    parser.add_argument("-o", required=True, type=Path)
    parser.add_argument("sources", nargs="+", type=Path)
    args = parser.parse_args()
    compile(args.arch, args.sources, args.o)
