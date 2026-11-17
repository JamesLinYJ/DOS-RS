#!/usr/bin/env python3
"""Build and install the native Rust ELF programs on the wide kernel's FAT boot volume."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROGRAMS = ("sh", "cat", "wc", "echo", "unlink")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=["i686", "x86_64"], required=True)
    parser.add_argument("--seed", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    env = os.environ.copy()
    # Inner Cargo builds target a userspace ABI and use a separate target directory.
    for key in ("CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS", "CARGO_MAKEFLAGS",
                "MAKEFLAGS", "DOSRS_UUTILS_ARCHIVE"):
        env.pop(key, None)
    env.setdefault("DOSRS_MUSL_ARCHIVE", str(ROOT / "target/upstream/musl-1.2.6.tar.gz"))
    with tempfile.TemporaryDirectory(dir=args.output.parent) as directory:
        work = Path(directory)
        binaries = work / "bin"
        subprocess.run([
            sys.executable, ROOT / "scripts/build-rust-tools.py",
            "--arch", args.arch, "--output", binaries,
        ], env=env, check=True)
        disk = work / "boot.img"
        shutil.copyfile(args.seed, disk)
        subprocess.run(["mmd", "-i", disk, "::bin"], check=True)
        for name in PROGRAMS:
            subprocess.run(["mcopy", "-i", disk, binaries / name, "::bin/" + name], check=True)
        os.replace(disk, args.output)


if __name__ == "__main__":
    main()
