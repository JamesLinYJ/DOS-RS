"""Boot an installed Chinese Windows 3.2 from native DOS and capture its enhanced-mode About box."""
import argparse
import hashlib
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import zlib

from qemu_shell import Shell


# Original Chinese 640x480 VGA glyphs. Resource counts and registration text vary.
DESKTOP = (
    ((270, 51, 362, 70), "a78b93aca022241b5b813d29fe19ac4abd4806cec1266b3cd55a16fbd94fd16f"),
    ((245, 94, 303, 110), "d933368b472aabf3f89729248538f41d15a990191ece877cf5cbcfc9a5a88e80"),
)
HELP_MENU = (
    ((275, 72, 334, 88), "15e206e11572d600ae96bdf98fe5679f158df20125aaa379d011fe21e398ed46"),
)
# About Program Manager title, version 3.2, and the complete '386 增强方式' text.
ABOUT = (
    ((282, 82, 415, 100), "84367ff197937ace1bd654a9174985360df014c68cd270ccadabd5a0e4eb0710"),
    ((197, 149, 257, 166), "904820b585ceefa9f6d7c5990328976685c1c74899cdc4b37f2f87dbdb016cd2"),
    ((197, 370, 299, 389), "53988a098b4508076329f465196763b2b35cc029f571f83a5bbb3f371e3a2a57"),
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
        if hashlib.sha256(area).hexdigest() != digest:
            return False
    return True


def wait_screen(vm, work, landmarks, name):
    path = work / "vga.ppm"
    deadline = time.monotonic() + 60
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


def boot(disk, ram_dir, proof, arch, cpu):
    with tempfile.TemporaryDirectory(dir=ram_dir) as directory:
        work = Path(directory)
        overlay = work / "c.qcow2"
        subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-F", "raw",
                        "-b", str(disk), str(overlay)], check=True)
        binary = "qemu-system-i386" if arch == "i686" else "qemu-system-x86_64"
        vm = Shell([binary, "-machine", "pc", "-accel", "tcg", "-cpu", cpu, "-m", "16",
                    "-boot", "c", "-no-reboot", "-vga", "std", "-drive",
                    f"if=ide,format=qcow2,file={overlay}"], proof / f"{arch}.serial", work)
        try:
            vm.wait(b"DOS / 80386")
            vm.wait(b"C:\\> ")
            vm.command("memstat /test", expect=b"XMS handle table allocation, lock, resize and release verified")
            vm.enter("cd windows")
            vm.enter("win /3")
            wait_screen(vm, work, DESKTOP, "Program Manager")
            vm.key("alt-h")
            wait_screen(vm, work, HELP_MENU, "Help menu")
            vm.key("a")
            pixels = wait_screen(vm, work, ABOUT, "Windows 3.2 enhanced-mode About box")
            dest = proof / f"native-windows32-enhanced-{arch}.png"
            png(dest, pixels)
            print(f"{arch} ({cpu}) WIN /3: {dest} {hashlib.sha256(dest.read_bytes()).hexdigest()}", flush=True)
        finally:
            vm.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disk", type=Path, required=True, help="native BIOS C disk that boots to the DOS prompt")
    parser.add_argument("--ram-dir", type=Path, required=True, help="mounted RAM disk for writable overlays")
    parser.add_argument("--proof", type=Path, required=True)
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch, cpu in [("i686", "pentium3"), ("x86_64", "qemu64")]:
        boot(args.disk.resolve(strict=True), args.ram_dir, args.proof, arch, cpu)
