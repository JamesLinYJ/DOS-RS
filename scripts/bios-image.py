#!/usr/bin/env python3
"""Build DOS-RS's BIOS disk from the same PE kernel that boots on UEFI."""

import argparse
import json
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def stages():
    upstream = ROOT / "target/bios-cpu"
    dest = ROOT / "target/bios"
    dest.mkdir(exist_ok=True)
    sysroot = subprocess.check_output(
        ["rustc", "--print", "sysroot"], text=True
    ).strip()
    objcopy = next(Path(sysroot).glob("lib/rustlib/*/bin/llvm-objcopy"))
    result = []
    for name, model in [
        ("mbr", "i386-code16-boot-sector"),
        ("loader", "i386-code16-boot-sector"),
        ("enter", "i686-stage-3"),
    ]:
        target = dest / (model + ".json")
        spec = json.loads((upstream / target.name).read_text())
        # Modern rustc enforces the floating-point ABI of custom x86 targets.
        spec.update({"rustc-abi": "softfloat", "features": "+soft-float,-sse,-mmx"})
        contents = json.dumps(spec, indent=2)
        if not target.exists() or target.read_text() != contents:
            target.write_text(contents)
        subprocess.run(
            [
                "cargo",
                "build",
                "--locked",
                "--manifest-path",
                ROOT / "boot/bios/Cargo.toml",
                "--release",
                "--bin",
                name,
                "-Zjson-target-spec",
                "--target",
                target,
                "--target-dir",
                dest,
            ],
            cwd=ROOT,
            check=True,
        )
        binary = dest / (name + ".bin")
        subprocess.run(
            [objcopy, "-O", "binary", dest / model / "release" / name, binary],
            check=True,
        )
        result.append(binary.read_bytes())
    return result


def image(kernel, output):
    boot, loader, transition = stages()
    pe = Path(kernel).read_bytes()
    offset = struct.unpack_from("<I", pe, 0x3C)[0]
    capacity = struct.unpack_from("<I", pe, offset + 24 + 56)[0]

    def padded(data):
        return data + bytes(-len(data) % 512)

    payload = (
        padded(struct.pack("<8sIII", b"DOSRSBI1", len(transition), len(pe), capacity))
        + padded(transition)
        + padded(pe)
    )
    assert len(boot) == 512 and boot[510:] == b"\x55\xaa"
    mbr = bytearray(boot)
    loader = padded(loader)
    start = 1 + len(loader) // 512
    for i, (kind, lba, count) in enumerate(
        [(0x20, 1, start - 1), (0xDA, start, len(payload) // 512)]
    ):
        struct.pack_into(
            "<B3xB3xII", mbr, 446 + 16 * i, 0x80 if i == 0 else 0, kind, lba, count
        )
    with output.open("wb") as out:
        for part in [mbr, loader, payload]:
            out.write(part)
        out.truncate(max(16 * 1024 * 1024, out.tell()))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kernel", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    image(args.kernel, args.output)
