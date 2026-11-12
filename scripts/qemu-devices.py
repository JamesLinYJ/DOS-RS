#!/usr/bin/env python3
"""Exercise CONFIG.SYS and the PC peripherals using isolated FAT images."""
import argparse
import os
from pathlib import Path
import shutil
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def prepare(output, setver):
    image = output / "drivers.img"
    shutil.copyfile(ROOT / "disks/guest12.img", image)

    def add(path, name):
        subprocess.run(["mcopy", "-o", "-i", image, path, "::" + name], check=True)

    directives = ["DEVICE=INVALID.SYS"]
    invalid = output / "INVALID.SYS"
    invalid.write_bytes(bytes(32))
    add(invalid, invalid.name)
    if setver:
        add(setver, "SETVER.SYS")
        directives.append("DEVICE=SETVER.SYS")
    fixtures = [("REJECT.SYS", ["-DCHARACTER", "-DREJECT"]), ("ECHO.SYS", ["-DCHARACTER", "-DCHAIN"]),
                ("BLOCK.SYS", ["-DBLOCK"]), ("PROBE.COM", ["-DSETVER"] if setver else [])]
    for filename, defines in fixtures:
        dest = output / filename
        subprocess.run(["nasm", "-f", "bin", *defines, ROOT / "tests/devices.asm", "-o", dest], check=True)
        if filename == "ECHO.SYS":
            payload = dest.read_bytes()
            size = 32 + len(payload)
            # An MZ overlay loads into the device segment without a PSP.
            header = struct.pack("<14H", 0x5a4d, size % 512, (size + 511) // 512,
                                 0, 2, 0, 0, 0, 0, 0, 0, 0, 28, 0)
            dest.write_bytes(header.ljust(32, bytes(1)) + payload)
        add(dest, filename)
    directives.extend(["DEVICE=REJECT.SYS", "DEVICEHIGH=ECHO.SYS", "DEVICE=BLOCK.SYS"])
    for filename, lines in [("CONFIG.SYS", directives), ("AUTOEXEC.BAT", ["PROBE.COM"])]:
        path = output / filename
        path.write_bytes(("\r\n".join(lines) + "\r\n").encode("ascii"))
        add(path, filename)
    return image


def probe(target, emulator, output, image, setver):
    name = target.split("-")[0]
    dest = output / name
    dest.mkdir(exist_ok=True)
    target_dir = ROOT / "target/device-probe"
    build = ["cargo", "build", "--release", "--features", "batch-console", "--target-dir", target_dir]
    if name == "i686":
        build.extend(["-Zjson-target-spec", "--target", "./i686-unknown-none.json"])
    else:
        build.extend(["--target", target])
    subprocess.run(build, cwd=ROOT, env={**os.environ, "DOS_RS_GUEST_IMAGE": str(image)}, check=True)
    console, serial, printer = [dest / file for file in ["console.txt", "com2.bin", "lpt1.bin"]]
    for path in [console, serial, printer]:
        path.write_bytes(b"")
    ide = dest / "ide.img"
    sector = b"IDE" + bytes(508) + bytes([0xa5])
    ide.write_bytes(bytes(7 * 512) + sector + bytes(8 * 512))
    command = [emulator, "-kernel", target_dir / target / "release/dos4-rs",
               "-display", "none", "-vga", "std", "-serial", f"file:{console}",
               "-serial", f"file:{serial}", "-parallel", f"file:{printer}",
               "-monitor", "stdio", "-nic", "none", "-no-reboot", "-m", "64",
               "-drive", "file=ide.img,format=raw,if=ide,index=0,snapshot=on",
               "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]
    with subprocess.Popen(command, cwd=dest, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL) as vm:
        try:
            deadline = time.monotonic() + 20
            while vm.poll() is None and time.monotonic() < deadline:
                if b"WAITING FOR PS2" in console.read_bytes():
                    vm.stdin.write(b"screendump vga.ppm\nsendkey a\n")
                    vm.stdin.flush()
                    break
                time.sleep(.04)
            status = vm.wait(timeout=5)
        finally:
            if vm.poll() is None:
                vm.kill()
    markers = ["Cannot load driver REJECT.SYS (DOS error 22)", "IDE LBA 7 OK", "Cannot load driver INVALID.SYS", "Loaded driver ECHO.SYS",
               "Loaded driver BLOCK.SYS", "CHAR AND BLOCK I/O OK", "DEVICE PROBE OK"]
    if setver:
        markers.append("Loaded driver SETVER.SYS")
    transcript = console.read_text()
    if (status != 33 or not all(mark in transcript for mark in markers)
            or serial.read_bytes() != b"SD" or printer.read_bytes() != b"P"):
        raise RuntimeError(f"{name} device regression failed\n{transcript}")
    print(f"{name} device probe passed. Evidence in {dest}", flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--setver", type=Path, help="also load the FreeDOS SETVER.SYS binary")
    cli.add_argument("--output", type=Path, default=ROOT / "target/device-proof")
    args = cli.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    image = prepare(output, args.setver)
    for target, emulator in [("i686-unknown-none", "qemu-system-i386"), ("x86_64-unknown-none", "qemu-system-x86_64")]:
        probe(target, emulator, output, image, args.setver)
