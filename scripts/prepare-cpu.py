#!/usr/bin/env python3
"""Prepare pinned architecture sources and the wide userspace libc archive."""

import argparse
import hashlib
import os
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
REV = "8628578dacd38b327d4f9acf820c6914f4b3a48f"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--from-checkout", type=Path)
parser.add_argument("--bios-checkout", type=Path)
parser.add_argument("--user32-checkout", type=Path)
parser.add_argument("--user64-checkout", type=Path)
parser.add_argument("--cpu-only", action="store_true", help="prepare the native 16-bit build without musl")
args = parser.parse_args()


def prepare(package, repo, revision, checkout):
    for line in (ROOT / f"scripts/{package}.sha256").read_text().splitlines():
        digest, name = line.split(maxsplit=1)
        dest = ROOT / "target" / package / name
        if dest.exists() and hashlib.sha256(dest.read_bytes()).hexdigest() == digest:
            continue
        if checkout:
            data = (checkout / name).read_bytes()
        else:
            with urllib.request.urlopen(
                f"https://raw.githubusercontent.com/{repo}/{revision}/{name}",
                timeout=30,
            ) as response:
                data = response.read()
        if hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"Unexpected upstream contents for {name}")
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)


prepare("edk2-cpu", "tianocore/edk2", REV, args.from_checkout)
prepare(
    "bios-cpu",
    "rust-osdev/bootloader",
    "3d7f72e36d4101ecf78b70cef2475d700bb01721",
    args.bios_checkout,
)

prepare("user32-cpu", "mit-pdos/xv6-public", "eeb7b415dbcb12cc362d0783e41c3d1f44066b17", args.user32_checkout)

prepare("user64-cpu", "mars-research/xv6-64", "1d89f61468ffe0ee975287712a2fb34fac4ba253", args.user64_checkout)


# The wide kernels ship a small /bin on their FAT boot volume. Its static
# programs link against the same pinned musl used by the installed Btrfs system.
musl = ROOT / "target/upstream/musl-1.2.6.tar.gz"
digest = "d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a"
if not args.cpu_only and (not musl.is_file() or hashlib.sha256(musl.read_bytes()).hexdigest() != digest):
    local = os.environ.get("DOSRS_MUSL_ARCHIVE")
    if local:
        data = Path(local).read_bytes()
    else:
        with urllib.request.urlopen("https://musl.libc.org/releases/musl-1.2.6.tar.gz", timeout=60) as response:
            data = response.read()
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("unexpected musl 1.2.6 archive checksum")
    musl.parent.mkdir(parents=True, exist_ok=True)
    musl.write_bytes(data)
