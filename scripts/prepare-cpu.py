#!/usr/bin/env python3
"""Fetch the pinned upstream CPU library. Builds never download sources."""

import argparse
import hashlib
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
REV = "8628578dacd38b327d4f9acf820c6914f4b3a48f"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--from-checkout", type=Path)
parser.add_argument("--bios-checkout", type=Path)
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
