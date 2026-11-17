#!/usr/bin/env python3
"""Link Rust ELF objects to the DOS-RS musl CRT and static syscall library."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    arch = os.environ["DOSRS_RUST_ARCH"]
    if arch not in ("i686", "x86_64"):
        raise SystemExit("unsupported Rust ELF architecture")
    libc = ROOT / "target/musl" / arch / "sysroot/lib"
    # rustc supplies -lc and several Linux libraries that live in the single
    # musl libc.a. panic=abort does not use the system unwinder.
    platform = {"-lc", "-lgcc_s", "-lgcc", "-lunwind", "-lpthread", "-lm",
                "-lutil", "-lrt", "-ldl", "-pie", "-static-pie", "-Bdynamic"}
    args = [arg for arg in sys.argv[1:] if arg not in platform]
    gcc = subprocess.check_output([arch + "-elf-gcc", "-print-libgcc-file-name"], text=True).strip()
    return subprocess.call([
        arch + "-elf-ld", "-static", "-z", "max-page-size=4096", "-T", ROOT / "ports/musl/user.ld",
        libc / "crt1.o", *args, "--start-group", libc / "libc.a", gcc, "--end-group"
    ])


if __name__ == "__main__":
    sys.exit(main())
