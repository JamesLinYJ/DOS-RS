#!/usr/bin/env python3
"""Read known bytes over high-speed USB and exchange Ethernet frames with QEMU."""

import argparse
from pathlib import Path
import shutil
import subprocess
import qemu


def run(arch, mode, root):
    work = root / f"{arch}-{mode}"
    work.mkdir(parents=True, exist_ok=True)
    seed = work / "dos.img"
    shutil.copyfile(qemu.ROOT / "disks/guest12.img", seed)
    batch = work / "AUTOEXEC.BAT"
    batch.write_bytes(
        b"HW\r\nUSB\r\nUSB READ 7\r\nNET\r\nNET ARP 10.0.2.15 10.0.2.2\r\nBT\r\nWIFI\r\n"
    )
    subprocess.run(["mcopy", "-o", "-i", seed, batch, "::"], check=True)
    payload = b"DOS-RS reads USB 2.0 sector 0007"
    usb = work / "stick.img"
    with usb.open("wb") as f:
        f.truncate(2 * 1024 * 1024)
        f.seek(7 * 512)
        f.write(payload)
    kernel = qemu.build(arch, qemu.ROOT / "target/hardware", seed, "batch-console")
    log = work / "serial.txt"
    cmd = qemu.boot_args(kernel, arch, work, 4, mode) + [
        "-display",
        "none",
        "-monitor",
        "none",
        "-serial",
        f"file:{log}",
        "-device",
        "qemu-xhci,id=xhci,p3=0,p2=4",
        "-drive",
        f"if=none,id=stick,format=raw,readonly=on,file={usb}",
        "-device",
        "usb-storage,bus=xhci.0,drive=stick",
        "-netdev",
        "user,id=lan",
        "-device",
        "e1000,netdev=lan",
        "-device",
        "isa-debug-exit,iobase=0xf4,iosize=0x04",
    ]
    result = subprocess.run(cmd, timeout=45)
    data = log.read_text(errors="replace")
    for marker in [
        "CPUs online: 4 / 4",
        "high-speed (480 Mbps)",
        "USB sector 7  " + payload.hex(),
        "ARP reply 52:55:0a:00:02:02",
        "Batch complete.",
    ]:
        if marker not in data:
            raise RuntimeError(f"{arch}/{mode} missing {marker}\n{data}")
    if result.returncode != 33:
        raise RuntimeError(f"QEMU exited with {result.returncode}")
    print(
        f"{arch}/{mode} read sector 7 via USB 2.0 and resolved 10.0.2.2 over Ethernet",
        flush=True,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, default=qemu.ROOT / "target/hardware-proof"
    )
    args = parser.parse_args()
    for mode in ["bios", "uefi"]:
        for arch in qemu.ARCH:
            run(arch, mode, args.output.resolve())
