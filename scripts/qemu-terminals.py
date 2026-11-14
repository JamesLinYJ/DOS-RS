#!/usr/bin/env python3
"""Run simultaneous DOS programs on four virtual terminals on both targets."""

import argparse
import os
import re
import qemu
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def prepare(out):
    image = out / "terminals.img"
    shutil.copyfile(ROOT / "disks/guest12.img", image)
    for name in ["SPIN", "WAITKEY", "CRASH", "CALC"]:
        program = out / (name + ".COM")
        subprocess.run(
            [
                "nasm",
                "-f",
                "bin",
                "-D" + name,
                ROOT / "tests/terminals.asm",
                "-o",
                program,
            ],
            check=True,
        )
        subprocess.run(["mcopy", "-o", "-i", image, program, "::"], check=True)
    return image


def check(out, image, arch, display, cpus, firmware):
    kernel = qemu.build(arch, ROOT / "target/terminals", image)
    log = out / (arch + ".txt")
    log.write_bytes(b"")
    cmd = qemu.boot_args(kernel, arch, out / (arch + "-boot"), cpus, firmware)
    cmd += [
        "-display",
        display,
        "-vga",
        "std",
        "-serial",
        "file:" + str(log),
        "-monitor",
        "stdio",
        "-nic",
        "none",
    ]
    vm = subprocess.Popen(
        cmd, cwd=out, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL
    )

    def monitor(command):
        vm.stdin.write((command + "\n").encode())
        vm.stdin.flush()

    def key(name):
        monitor("sendkey " + name + " 15")
        time.sleep(0.035)

    def enter(line):
        for char in line:
            key({".": "dot", " ": "spc"}.get(char, char))
        key("ret")

    def wait(marker, count=1):
        end = time.monotonic() + 12
        while time.monotonic() < end:
            if log.read_bytes().count(marker.encode()) >= count:
                return
            if vm.poll() is not None:
                break
            time.sleep(0.03)
        raise RuntimeError(f"{arch} missing {marker!r}\n{log.read_bytes()[-1600:]!r}")

    try:
        wait(f"CPUs online: {cpus} / {cpus}")
        wait("Type MEM for memory or HW for hardware.")
        wait("A:\\> ")
        enter("spin.com")
        wait("BACKGROUND RUNNING")
        key("alt-f2")
        wait("DOS-RS terminal 2")
        enter("waitkey.com")
        wait("WAITING FOR A")
        key("alt-f3")
        wait("DOS-RS terminal 3")
        enter("crash.com")
        wait("Program stopped at opcode 63")
        key("alt-f4")
        wait("DOS-RS terminal 4")
        enter("calc.com")
        wait("CALCULATION COMPLETE")
        wait("BACKGROUND SAW FILE")
        key("alt-f2")
        key("a")
        wait("KEY DELIVERED")
        enter("type shared.txt")
        wait("VT shared file")
        enter("hw")
        wait("PCI 00:")
        stats = re.findall(
            rb"CPU (\d+)  APIC (\d+)  guest slices (\d+)", log.read_bytes()
        )
        if len({apic for _, apic, _ in stats}) != cpus:
            raise RuntimeError(f"{arch} missing distinct APIC IDs: {stats}")
        if any(int(count) == 0 for slot, _, count in stats if 0 < int(slot) <= 4):
            raise RuntimeError(f"{arch} idle terminal worker: {stats}")
        key("alt-f4")
        enter("color 1e")
        enter("mem")
        free_kib = int(os.environ.get("DOS_RS_EXT_MIB", "8")) * 1024 - 64
        wait(f"Free XMS:     {free_kib} KiB")
        wait(f"Free EMS:     {free_kib // 16} pages of 16 KiB")
        enter("ver")
        wait("DOS-RS (DOS 4.00)")
        (out / (arch + ".ppm")).unlink(missing_ok=True)
        monitor(f"screendump {arch}.ppm")
        end = time.monotonic() + 3
        frame = out / (arch + ".ppm")
        while time.monotonic() < end:
            if frame.exists():
                header = frame.read_bytes().split(b"\n", 3)
                if len(header) == 4 and header[0] == b"P6":
                    width, height = map(int, header[1].split())
                    if len(header[3]) == width * height * 3:
                        break
            time.sleep(0.02)
        else:
            raise RuntimeError(f"{arch} incomplete QEMU screendump")
        from collections import Counter

        colors = Counter(zip(*[iter(header[3])] * 3))
        blue = sum(
            n for (r, g, b), n in colors.items() if r < 32 and g < 32 and b > 120
        )
        yellow = sum(
            n for (r, g, b), n in colors.items() if r > 190 and g > 190 and b < 150
        )
        if blue < 10000 or yellow < 100:
            raise RuntimeError(f"COLOR 1E failed: {colors.most_common(8)}")
        print(
            f"{arch}/{firmware}: four terminals, {blue} blue and {yellow} yellow pixels",
            flush=True,
        )
    finally:
        if vm.poll() is None:
            monitor("quit")
            vm.wait(timeout=5)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--output", type=Path, default=ROOT / "target/terminal-proof")
    cli.add_argument("--display", default="none")
    cli.add_argument("--cpus", type=int, default=4)
    cli.add_argument("--firmware", choices=["uefi", "bios"], default="uefi")
    args = cli.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    image = prepare(out)
    for arch in qemu.ARCH:
        check(out, image, arch, args.display, args.cpus, args.firmware)
