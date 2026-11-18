#!/usr/bin/env python3
"""Build and install the native Rust ELF programs on the wide kernel's FAT boot volume."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROGRAMS = ("sh", "cat", "wc", "echo", "unlink")


def populate(work, seed, binaries):
    disk = work / "boot.img"
    # Reformat the embedded distribution volume using enough cylinders for the
    # seed and the linked applications. The mountable source image is read-only.
    track = 36 * 2 * 512
    payload = seed.stat().st_size + sum((binaries / name).stat().st_size for name in PROGRAMS)
    cylinders = max(80, (payload + track - 1) // track + 2)
    (work / "seed.img").symlink_to(seed.resolve())
    with seed.open("rb") as reader:
        (work / "bootsector").write_bytes(reader.read(512))
    config = work / "drives.conf"
    config.write_text('drive s: file="seed.img"\ndrive t: file="boot.img"\n')
    env = dict(os.environ, MTOOLSRC=str(config.resolve()))
    def run(args):
        subprocess.run(args, cwd=work, env=env, check=True)
    run(["mformat", "-C", "-t", str(cylinders), "-h", "2", "-n", "36",
         "-B", "bootsector", "-v", "DOSRS", "t:"])
    run(["mcopy", "-smp", "s:/*", "t:/"])
    run(["mmd", "t:/bin"])
    for name in PROGRAMS:
        run(["mcopy", str((binaries / name).resolve()), "t:/bin/" + name])
    return disk


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
        disk = populate(work, args.seed, binaries)
        os.replace(disk, args.output)


if __name__ == "__main__":
    main()
