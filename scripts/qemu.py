#!/usr/bin/env python3
"""Build and boot DOS-RS in QEMU using BIOS or UEFI."""

import argparse
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ARCH = {"i686": ("i386", "BOOTIA32.EFI"), "x86_64": ("x86_64", "BOOTX64.EFI")}


def firmware(machine):
    override = os.environ.get("DOS_RS_FIRMWARE_" + machine.upper())
    if override:
        return Path(override)
    binary = Path(shutil.which("qemu-system-" + machine)).resolve()
    for base in [
        binary.parent.parent / "share/qemu",
        Path("/usr/share/qemu"),
        Path("/opt/homebrew/share/qemu"),
    ]:
        path = base / f"edk2-{machine}-code.fd"
        if path.exists():
            return path
    raise FileNotFoundError(
        "Set DOS_RS_FIRMWARE_" + machine.upper() + " to the OVMF code image"
    )


def build(arch, target_dir, image=None, features=None):
    target = arch + "-unknown-uefi"
    args = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "--target",
        target,
        "--target-dir",
        target_dir,
    ]
    if features:
        args.extend(["--features", features])
    env = os.environ.copy()
    if image:
        env["DOS_RS_GUEST_IMAGE"] = str(image)
    subprocess.run(args, cwd=ROOT, env=env, check=True)
    return target_dir / target / "release/dos4-rs.efi"


def boot_args(kernel, arch, work, cpus=4, mode="uefi"):
    machine, name = ARCH[arch]
    work.mkdir(parents=True, exist_ok=True)
    # SizeOfImage includes all four session arenas even when the PE file is small.
    with kernel.open("rb") as file:
        file.seek(0x3C)
        pe = struct.unpack("<I", file.read(4))[0]
        file.seek(pe + 24 + 56)
        image_bytes = struct.unpack("<I", file.read(4))[0]
    memory = str(
        max(256, ((image_bytes + (192 << 20) + (63 << 20)) // (64 << 20)) * 64)
    )
    if mode == "bios":
        disk = work / "bios.img"
        subprocess.run(
            [sys.executable, ROOT / "scripts/bios-image.py", kernel, disk], check=True
        )
        return [
            "qemu-system-" + machine,
            "-m",
            memory,
            "-smp",
            str(cpus),
            "-no-reboot",
            "-drive",
            f"if=none,id=bootdisk,format=raw,snapshot=on,file={disk}",
            "-device",
            "virtio-blk-pci,drive=bootdisk,bootindex=1",
        ]
    disk = work / "efi.img"
    with disk.open("wb") as stream:
        stream.truncate(64 * 1024 * 1024)
    subprocess.run(["mformat", "-F", "-i", disk, "::"], check=True)
    subprocess.run(["mmd", "-i", disk, "::EFI", "::EFI/BOOT"], check=True)
    subprocess.run(["mcopy", "-i", disk, kernel, "::EFI/BOOT/" + name], check=True)
    code = firmware(machine)
    template = Path(
        os.environ.get("DOS_RS_FIRMWARE_VARS", str(code.parent / "edk2-i386-vars.fd"))
    )
    variables = work / "efi-vars.fd"
    shutil.copyfile(template, variables)
    # The system's IDE master remains available for a user's persistent disk.
    return [
        "qemu-system-" + machine,
        "-smp",
        str(cpus),
        "-m",
        memory,
        "-drive",
        f"if=pflash,unit=0,format=raw,readonly=on,file={code}",
        "-drive",
        f"if=pflash,unit=1,format=raw,file={variables}",
        "-drive",
        f"if=none,id=esp,format=raw,readonly=on,file={disk}",
        "-device",
        "virtio-blk-pci,drive=esp,bootindex=1",
        "-no-reboot",
    ]


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=ARCH, default="x86_64")
    cli.add_argument("--firmware", choices=["uefi", "bios"], default="uefi")
    cli.add_argument("--cpus", type=int, default=4)
    cli.add_argument("--display", default=None)
    cli.add_argument("--kernel", type=Path)
    cli.add_argument("--extended-mib", type=int, help="extended MiB per DOS session (1..128)")
    cli.add_argument("--image", type=Path)
    cli.add_argument("extra", nargs=argparse.REMAINDER)
    args = cli.parse_args()
    if args.extended_mib is not None:
        if args.kernel or not 1 <= args.extended_mib <= 128:
            cli.error("--extended-mib requires building a kernel and a value from 1 to 128")
        os.environ["DOS_RS_EXT_MIB"] = str(args.extended_mib)
    kernel = args.kernel or build(args.arch, ROOT / "target", args.image)
    with tempfile.TemporaryDirectory() as temp:
        cmd = boot_args(kernel, args.arch, Path(temp), args.cpus, args.firmware)
        if args.display:
            cmd += ["-display", args.display]
        extra = args.extra[1:] if args.extra[:1] == ["--"] else args.extra
        return subprocess.call(
            cmd + (extra or ["-serial", "stdio", "-monitor", "none", "-nic", "none"])
        )


if __name__ == "__main__":
    raise SystemExit(main())
