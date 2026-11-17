"""Boot an installed Chinese Windows 3.2 from native DOS and capture its enhanced-mode About box."""
import argparse
import hashlib
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time
import zlib

import qemu
from qemu_shell import Shell


# The VGA DAC and in-tree VGA differ slightly in palette intensity.
# Compare the upper nibble of each RGB channel while retaining every glyph pixel.
PALETTE = bytes(value & 0xf0 for value in range(256))

# Original Chinese 640x480 VGA glyphs. Window positions, resource counts and registration vary.
DESKTOP = (
    ((274, 53, 354, 69), "9f3e6f11d3178c8d083089ad18bc16ee2048ed128cdf1cdd5255ed672ae88d9d"),
)
HELP_MENU = (
    ((285, 175, 430, 192), "e181e25ee55c2d2ba44d8e2cb42fd12b058815a1a0aa84ffdf4a456a4e14e4e1"),
)
# About Program Manager title, version 3.2, and the complete '386 增强方式' text.
ABOUT = (
    ((282, 82, 415, 100), "89c9400913d6ce5ba13e744d3f17dbe3dbdbfab0865871b1c91930141181b4ef"),
    ((197, 149, 257, 166), "9d24ebaf319422af908a747a893b0bd3ccd2541d6428dbce20f5a8d4a3c1bd7a"),
    ((197, 370, 299, 389), "5eb4868f2610bd39a99a9fc5ce6c4c443c51cfe511c47e5d22c99bf412846715"),
    # Wait for the resource label and OK button to finish painting.
    ((197, 409, 265, 430), "1100cfd313205ef9ac15f06dd1e72ba67e541868d3e6f39a783a89baf0828a9e"),
    ((480, 109, 566, 141), "f0ccfc0483f0ff83de7045ebf56309ff0a85eac680c281fd6afae3b040e2a7a4"),
)

# Locate a short row of glyph pixels, then verify every complete rectangle at that displacement.
def glyph_row(bits, background):
    return b"".join(b"\xf0\xf0\xf0" if bit == "1" else background for bit in bits)


ANCHORS = {
    DESKTOP: (274, 53, glyph_row("00001000000001000000", b"\x00\x00\xa0")),
    HELP_MENU: (314, 176, glyph_row("10111111110111111110", b"\x00\x00\x00")),
    ABOUT: (282, 83, glyph_row("00000000000100000001", b"\x00\x00\xa0")),
}


def png(dest, pixels):
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))
    stride = 640 * 3
    rows = b"".join(b"\0" + pixels[i:i + stride] for i in range(0, len(pixels), stride))
    header = struct.pack(">IIBBBBB", 640, 480, 8, 2, 0, 0, 0)
    dest.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                     + chunk(b"IDAT", zlib.compress(rows, 4)) + chunk(b"IEND", b""))


def matches(pixels, landmarks, dx, dy):
    for (x, y, right, bottom), digest in landmarks:
        x, right = x + dx, right + dx
        y, bottom = y + dy, bottom + dy
        if x < 0 or right > 640 or y < 0 or bottom > 480:
            return False
        area = b"".join(pixels[3 * (row * 640 + x):3 * (row * 640 + right)]
                        for row in range(y, bottom))
        if hashlib.sha256(area).hexdigest() != digest:
            return False
    return True


def visible(pixels, landmarks):
    pixels = pixels.translate(PALETTE)
    anchor_x, anchor_y, row = ANCHORS[landmarks]
    offset = pixels.find(row)
    while offset >= 0:
        y, x = divmod(offset // 3, 640)
        if offset % 3 == 0 and matches(pixels, landmarks, x - anchor_x, y - anchor_y):
            return True
        offset = pixels.find(row, offset + 1)
    return False


def wait_screen(vm, work, landmarks, name, timeout):
    path = work / "vga.ppm"
    deadline = time.monotonic() + timeout
    while vm.vm.poll() is None and time.monotonic() < deadline:
        path.unlink(missing_ok=True)
        vm.monitor(f'screendump "{path}"')
        for _ in range(20):
            time.sleep(0.05)
            if path.exists():
                parts = path.read_bytes().split(b"\n", 3)
                if len(parts) == 4:
                    if parts[:3] == [b"P6", b"640 480", b"255"] and len(parts[3]) == 640 * 480 * 3:
                        if visible(parts[3], landmarks):
                            return parts[3]
                        break
                    if parts[1] != b"640 480":
                        break
        time.sleep(0.1)
    if path.is_file():
        shutil.copyfile(path, vm.log.with_suffix(".last-screen.ppm"))
    raise RuntimeError(f"did not see {name}; serial log: {vm.log}")


def boot(disk, ram_dir, proof, arch, cpu=None):
    with tempfile.TemporaryDirectory(dir=ram_dir) as directory:
        work = Path(directory)
        overlay = work / "c.qcow2"
        subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-F", "raw",
                        "-b", str(disk), str(overlay)], check=True)
        if cpu:
            binary = "qemu-system-i386" if arch == "i686" else "qemu-system-x86_64"
            label = f"16-bit DOS / 80386 on {binary} -cpu {cpu}"
            args = [binary, "-machine", "pc", "-accel", "tcg", "-cpu", cpu, "-m", "16",
                    "-boot", "c", "-no-reboot", "-vga", "std"]
            stem = "native-windows32-enhanced-" + arch
            log = proof / f"{arch}.serial"
        else:
            firmware = "bios" if arch == "i686" else "uefi"
            label = f"{arch} kernel / {firmware}"
            kernel = qemu.build(arch, qemu.ROOT / "target/windows-verification", features="pc386")
            args = qemu.boot_args(kernel, arch, work / "boot", mode=firmware)
            stem = f"{arch}-{firmware}-windows32-enhanced"
            log = proof / f"{arch}-{firmware}.serial"
        vm = Shell(args + ["-drive", f"if=ide,index=0,format=qcow2,file={overlay}"], log, work)
        try:
            if cpu:
                vm.wait(b"DOS / 80386")
                vm.wait(b"C:\\> ")
                vm.command("memstat /test", expect=b"XMS handle table allocation, lock, resize and release verified")
            else:
                vm.wait(b"DOS / " + arch.encode())
                vm.wait(b"Legacy BIOS" if arch == "i686" else b"UEFI boot services exited.")
                vm.wait(b"A:\\> ")
                vm.command("c:")
            vm.command("cd windows", prompt=b"C:\\WINDOWS> ")
            vm.enter("win /3")
            timeout = 60 if cpu else 600
            wait_screen(vm, work, DESKTOP, "Program Manager", timeout)
            vm.monitor("sendkey alt-h 80")
            wait_screen(vm, work, HELP_MENU, "Help menu", timeout)
            vm.monitor("sendkey a 80")
            pixels = wait_screen(vm, work, ABOUT, "Windows 3.2 enhanced-mode About box", timeout)
            dest = proof / (stem + ".png")
            png(dest, pixels)
            print(f"{label}: WIN /3, 386 enhanced mode; "
                  f"{dest} {hashlib.sha256(dest.read_bytes()).hexdigest()}", flush=True)
        finally:
            vm.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", choices=("native16", "i686", "x86_64"), default="native16")
    parser.add_argument("--disk", type=Path, required=True, help="raw installed Windows C disk; native16 needs a bootable MBR")
    parser.add_argument("--ram-dir", type=Path, required=True, help="mounted RAM disk for writable overlays")
    parser.add_argument("--proof", type=Path, required=True)
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    disk = args.disk.resolve(strict=True)
    if args.kernel == "native16":
        for arch, cpu in [("i686", "pentium3"), ("x86_64", "qemu64")]:
            boot(disk, args.ram_dir, args.proof, arch, cpu)
    else:
        boot(disk, args.ram_dir, args.proof, args.kernel)
