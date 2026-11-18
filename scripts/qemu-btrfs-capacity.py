#!/usr/bin/env python3
"""Measure native statvfs and force Btrfs to commit a new data group on a RAM-backed GPT disk."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from uuid import UUID
from zlib import crc32

import qemu
from qemu_shell import Shell

MIB = 1 << 20
ROOT = 8 * MIB
SEED = 32 * 1024
BASIC = UUID("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7").bytes_le
LINUX = UUID("0fc63daf-8483-4772-8e79-3d69d8477de4").bytes_le


def partition(disk, root_len, readonly):
    sectors = disk.stat().st_size // 512
    entries = bytearray(128 * 128)
    for i, (kind, pos, size) in enumerate(
        [(BASIC, MIB, 4 * MIB), (LINUX, ROOT, root_len)]
    ):
        struct.pack_into(
            "<16s16sQQQ",
            entries,
            i * 128,
            kind,
            UUID(int=i + 1).bytes_le,
            pos // 512,
            (pos + size) // 512 - 1,
            int(readonly) << 60,
        )
    mbr = bytearray(512)
    struct.pack_into("<B3xB3xII", mbr, 446, 0, 0xEE, 1, sectors - 1)
    mbr[510:] = b"\x55\xaa"
    with disk.open("r+b") as out:
        out.write(mbr)
        for lba, other, table in [(1, sectors - 1, 2), (sectors - 1, 1, sectors - 33)]:
            header = bytearray(512)
            struct.pack_into(
                "<8sIIIIQQQQ16sQIII",
                header,
                0,
                b"EFI PART",
                0x10000,
                92,
                0,
                0,
                lba,
                other,
                34,
                sectors - 34,
                UUID(int=3).bytes_le,
                table,
                128,
                128,
                crc32(entries),
            )
            struct.pack_into("<I", header, 16, crc32(header[:92]))
            out.seek(lba * 512)
            out.write(header)
            out.seek(table * 512)
            out.write(entries)


def helper(verb, *args):
    subprocess.run(
        [sys.executable, qemu.ROOT / "scripts/linux-btrfs.py", verb, *args], check=True
    )


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as src:
        for data in iter(lambda: src.read(MIB), b""):
            h.update(data)
    return h.hexdigest()


def dump(root, work, proof, label):
    output = work / ("linux-" + label)
    helper("check", "--image", root, "--output", output)
    for name in ["chunks.txt", "check.log"]:
        shutil.copyfile(output / name, proof / (label + "-" + name))
    chunks = (output / "chunks.txt").read_text()
    groups = {
        kind: [
            int(n)
            for n in re.findall(
                rf"length (\d+) owner \d+ stripe_len \d+ type {kind}(?:\||\s)", chunks
            )
        ]
        for kind in ("DATA", "METADATA")
    }
    data = digest(output / "files/large.bin")
    if (
        not all(groups.values())
        or "no error found" not in (output / "check.log").read_text()
    ):
        raise RuntimeError("Linux could not confirm the data chunk list and checksums")
    shutil.rmtree(output)
    return groups, data


def test(arch, ram, proof, metadata_pressure):
    proof = proof / arch
    proof.mkdir(parents=True, exist_ok=True)
    kernel = qemu.build(arch, qemu.ROOT / "target")
    with tempfile.TemporaryDirectory(dir=ram) as temp:
        work = Path(temp)
        tree = work / "seed"
        tree.mkdir()
        seed = bytes((i * 37 + 9) & 255 for i in range(256)) * (SEED // 256)
        (tree / "large.bin").write_bytes(seed)
        root = work / "root.img"
        nodesize = 16384
        if metadata_pressure:
            # Btrfs stores each short file inline in a tree leaf. Large nodes
            # also need more bytes of scratch space to commit new chunk metadata.
            nodesize = 65536
            metadata = tree / "metadata-seed"
            metadata.mkdir()
            for i in range(1100):
                (metadata / str(i)).write_bytes(bytes([i % 256]) * 1536)
        helper("create", "--nodesize", str(nodesize), "--root", tree, "--image", root)
        before, baseline = dump(root, work, proof, "before")
        if before["DATA"] != [8 * MIB] or baseline != hashlib.sha256(seed).hexdigest():
            raise RuntimeError(
                f"fixture differs from the original 8 MiB data group: {before}"
            )
        if metadata_pressure and before["METADATA"] != [8 * MIB]:
            raise RuntimeError(
                f"fixture must start with one metadata block group: {before}"
            )
        root_len = root.stat().st_size
        fat = work / "fat.img"
        with fat.open("xb") as out:
            out.truncate(4 * MIB)
        subprocess.run(["mformat", "-N", "0x12345678", "-i", fat, "::"], check=True)
        elf = work / "capacity.elf"
        subprocess.run(
            [
                sys.executable,
                qemu.ROOT / "scripts/native-cc.py",
                "--arch",
                arch,
                "-o",
                elf,
                qemu.ROOT / "native-exec/examples/capacity.c",
            ],
            check=True,
        )
        subprocess.run(["mcopy", "-i", fat, elf, "::"], check=True)
        disk = work / "disk.img"
        with disk.open("xb") as out:
            out.truncate(ROOT + root_len + MIB)
            for pos, src in [(MIB, fat), (ROOT, root)]:
                out.seek(pos)
                with src.open("rb") as f:
                    shutil.copyfileobj(f, out)
        root.unlink()
        partition(disk, root_len, False)
        for mode, msg in [
            ("write metadata", "metadata pressure write OK")
            if metadata_pressure
            else ("", "allocated data across original block group OK"),
            (
                "cold metadata" if metadata_pressure else "cold",
                "cold block group file OK",
            ),
            ("ro", "read-only FAT and Btrfs capacity OK"),
        ]:
            if mode == "ro":
                partition(disk, root_len, True)
                expected = digest(disk)
            vm = Shell(
                qemu.boot_args(
                    kernel,
                    arch,
                    work / "boot",
                    4,
                    "bios" if arch == "i686" else "uefi",
                    esp_mib=36,
                )
                + ["-drive", f"if=ide,index=0,format=raw,file={disk}"],
                proof / ((mode or "write").split()[0] + ".serial"),
                work,
            )
            try:
                vm.wait(b"Mounted Btrfs /")
                vm.wait(b"A:\\> ")
                vm.command("c:")
                begin = vm.log.stat().st_size
                vm.enter("capacity.elf" + (" " + mode if mode else ""))
                vm.wait(b"C:\\> ", since=begin, timeout=900)
                serial = vm.log.read_bytes()[begin:]
                if msg.encode() not in serial:
                    raise RuntimeError(serial.decode(errors="replace"))
            finally:
                vm.close()
            if mode == "ro" and expected != digest(disk):
                raise RuntimeError("read-only GPT disk changed")
        with disk.open("rb") as src, root.open("xb") as out:
            src.seek(ROOT)
            for _ in range(root_len // MIB):
                out.write(src.read(MIB))
        disk.unlink()
        after, actual = dump(root, work, proof, "after")
        wanted = hashlib.sha256(seed)
        size = len(seed)
        for page in range(1 if metadata_pressure else 36):
            data = bytes((i * 7 + page * 31) & 255 for i in range(256)) * 1024
            wanted.update(data)
            size += len(data)
        want = wanted.hexdigest()
        kinds = ("METADATA",) if metadata_pressure else ("DATA",)
        if actual != want or any(sum(after[k]) <= sum(before[k]) for k in kinds):
            raise RuntimeError(
                f"allocation or restored file mismatch: {before}, {after}, {actual}, {want}"
            )
        report = dict(
            arch=arch,
            original_data_groups=before["DATA"],
            original_metadata_groups=before["METADATA"],
            final_data_groups=after["DATA"],
            final_metadata_groups=after["METADATA"],
            bytes=size,
            expected_sha256=want,
            restored_sha256=actual,
            readonly_disk_sha256=expected,
        )
        (proof / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report), flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", action="append", choices=["i686", "x86_64"])
    cli.add_argument(
        "--metadata-pressure",
        action="store_true",
        help="exercise the initial metadata reserve using inline files and 64 KiB tree nodes",
    )
    cli.add_argument("--ram-dir", type=Path, required=True)
    cli.add_argument("--proof", type=Path, required=True)
    args = cli.parse_args()
    for arch in args.arch or ["i686", "x86_64"]:
        test(arch, args.ram_dir, args.proof.resolve(), args.metadata_pressure)
