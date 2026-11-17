"""Boot an installed Chinese Windows 3.2 from native DOS and capture its enhanced-mode About box."""
import argparse
import hashlib
from pathlib import Path
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

# Original Chinese 640x480 VGA glyphs. Resource counts and registration text vary.
DESKTOP = (
    ((270, 51, 362, 70), "3f1ea037981b4f1e26b5e926182c8e70cd5ca74630e6b2ea903458a5e875d28f"),
    ((245, 94, 303, 110), "b212497c9138af24b02ec24f257ea8467d9e283afb05d7f228b5875501d9e394"),
)
HELP_MENU = (
    ((275, 72, 334, 88), "026481c47a5b2e0972ef9477ee4c3c29f261d6438f40b0570d5cf166805a6db9"),
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


def png(dest, pixels):
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))
    stride = 640 * 3
    rows = b"".join(b"\0" + pixels[i:i + stride] for i in range(0, len(pixels), stride))
    header = struct.pack(">IIBBBBB", 640, 480, 8, 2, 0, 0, 0)
    dest.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                     + chunk(b"IDAT", zlib.compress(rows, 4)) + chunk(b"IEND", b""))


def visible(pixels, landmarks):
    for (x, y, right, bottom), digest in landmarks:
        area = b"".join(pixels[3 * (row * 640 + x):3 * (row * 640 + right)]
                        for row in range(y, bottom))
        if hashlib.sha256(area.translate(PALETTE)).hexdigest() != digest:
            return False
    return True


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
            timeout = 60 if cpu else 240
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
