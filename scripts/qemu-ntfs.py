#!/usr/bin/env python3
"""Build a populated NTFS volume in RAM and verify both kernel entry points and write refusal."""
import argparse
import hashlib
from pathlib import Path
import shutil
import struct
import uuid
import zlib
import subprocess
import tempfile

import qemu
from qemu_shell import Shell

def make_volume(work):
    disk = work / "ntfs.img"
    with disk.open("xb") as out:
        out.truncate(16 << 20)
    subprocess.run(["mkntfs", "-Q", "-F", "-s", "512", "-L", "DOSRSNTFS", disk], check=True)
    payload = (b"abcdefghijklmnopqrstuvwxyz012345" * 512)
    hello = subprocess.check_output(["mtype", "-i", qemu.ROOT / "disks/guest12.img", "::hello.com"])
    for name, data in [("hello.com", hello), ("note.txt", b"native NTFS\r\n"), ("long sample.bin", payload)]:
        source = work / name
        source.write_bytes(data)
        subprocess.run(["ntfscp", disk, source, "/" + name], check=True)
        if subprocess.check_output(["ntfscat", disk, "/" + name]) != data:
            raise ValueError(f"NTFS fixture disagrees with ntfscat for {name}")
    for n in range(64):
        subprocess.run(["ntfscp", disk, work / "note.txt", f"/item{n:03}.txt"], check=True)
    return disk

def partitioned(volume, work, layout):
    if layout == "raw":
        return volume
    disk = work / f"{layout}.img"
    first = 2048
    sectors = volume.stat().st_size // 512
    total = first + sectors + 2048
    mbr = bytearray(512)
    mbr[510:] = b"\x55\xaa"
    entry = 446
    with disk.open("xb") as out:
        out.truncate(total * 512)
        if layout == "mbr":
            mbr[entry + 4] = 7
            struct.pack_into("<II", mbr, entry + 8, first, sectors)
        else:
            mbr[entry + 4] = 0xee
            struct.pack_into("<II", mbr, entry + 8, 1, total - 1)
            table = bytearray(128 * 128)
            table[:16] = uuid.UUID("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7").bytes_le
            table[16:32] = uuid.uuid4().bytes_le
            struct.pack_into("<QQ", table, 32, first, first + sectors - 1)
            guid = uuid.uuid4().bytes_le
            for pos, alternate, entries in [(1, total - 1, 2), (total - 1, 1, total - 33)]:
                header = bytearray(512)
                struct.pack_into("<8sIIIIQQQQ16sQIII", header, 0, b"EFI PART", 0x10000,
                                 92, 0, 0, pos, alternate, 34, total - 34, guid,
                                 entries, 128, 128, zlib.crc32(table))
                struct.pack_into("<I", header, 16, zlib.crc32(header[:92]))
                out.seek(pos * 512)
                out.write(header)
                out.seek(entries * 512)
                out.write(table)
        out.seek(0)
        out.write(mbr)
        out.seek(first * 512)
        with volume.open("rb") as src:
            shutil.copyfileobj(src, out)
    return disk

def run(arch, firmware, work, disk, log):
    kernel = qemu.build(arch, qemu.ROOT / "target")
    cmd = qemu.boot_args(kernel, arch, work / "boot", 4, firmware)
    cmd += ["-drive", f"if=ide,index=0,format=raw,file={disk}"]
    vm = Shell(cmd, log, work)
    try:
        vm.wait(b"Mounted NTFS C:\\ (read-only).")
        vm.wait(b"A:\\> ")
        vm.command("c:")
        vm.command("type note.txt", expect=b"native NTFS")
        vm.command('cat "LONG SAMPLE.BIN" | wc -c', expect=b"\r\n16384\r\n")
        vm.command("cat item063.txt", expect=b"native NTFS")
        vm.command("hello.com", expect=b"GUEST HELLO COM")
        vm.command("cat note.txt > count.txt", expect=b"read-only file system")
        vm.command("unlink note.txt", expect=b"read-only file system")
        vm.command("del note.txt")
        vm.command("cat note.txt", expect=b"native NTFS")
    finally:
        vm.close()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=qemu.ROOT / "target/ntfs-proof")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as temp:
        work = Path(temp)
        volume = make_volume(work)
        for layout in ("raw", "mbr", "gpt"):
            disk = partitioned(volume, work, layout)
            before = hashlib.sha256(disk.read_bytes()).hexdigest()
            for arch, firmware in [("i686", "bios"), ("x86_64", "uefi")]:
                run(arch, firmware, work, disk, args.proof / f"{arch}-{layout}.log")
                if hashlib.sha256(disk.read_bytes()).hexdigest() != before:
                    raise ValueError(f"{arch} modified the read-only volume")
                print(f"{arch}/{firmware}/{layout}: NTFS reads, DOS EXEC, read-only image SHA-256 {before}", flush=True)
if __name__ == "__main__":
    main()
