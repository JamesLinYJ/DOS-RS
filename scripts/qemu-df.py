#!/usr/bin/env python3
"""Compare the native df front end and Linux GNU df on the same FAT/Btrfs GPT image."""

import argparse
import gzip
import hashlib
import importlib.util
import json
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

import qemu
from qemu_shell import Shell


def module(name):
    spec = importlib.util.spec_from_file_location(
        name, Path(__file__).with_name(name + ".py")
    )
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


linux = module("linux-btrfs")
capacity = module("qemu-btrfs-capacity")
KERNEL_APK = "linux-virt-6.12.117-r0.apk"
KERNEL_SHA256 = "dc01531450938a224f8cc369da67197aad4919d0841fb80754c40e6b6fb6adab"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def oracle(disk, work, gnu, cache, log, warm=False):
    archive = linux.download(linux.REPO, KERNEL_APK, KERNEL_SHA256, cache)
    entries = linux.packages(cache)
    kernel = work / "vmlinuz"
    with tarfile.open(archive, "r:gz", ignore_zeros=True) as package:
        kernel.write_bytes(package.extractfile("boot/vmlinuz-virt").read())
        names = package.getnames()
        dep = next(name for name in names if name.endswith("/modules.dep"))
        lines = package.extractfile(dep).read().decode().splitlines()
        deps = {line.split(":")[0]: line.split(":")[1].split() for line in lines}
        modules = []

        def add(name):
            if name not in modules:
                for needed in deps[name]:
                    add(needed)
                modules.append(name)

        for target in [
            "virtio_mmio",
            "virtio_blk",
            "crc32c_generic",
            "btrfs",
            "nls_cp437",
            "nls_iso8859-1",
            "vfat",
        ]:
            for name in deps:
                if name.endswith("/" + target + ".ko.gz"):
                    add(name)
        commands = []
        for index, name in enumerate(modules):
            stored = str(Path(dep).parent / name)
            outfile = f"mods/{index}.ko"
            entries[outfile] = (
                stat.S_IFREG | 0o644,
                gzip.decompress(package.extractfile(stored).read()),
            )
            commands.append("echo Loading " + name + "; insmod /" + outfile)
    for d in [
        "mods",
        "mnt",
        "mnt/btrfs",
        "mnt/fat",
        "dev",
        "proc",
        "sys",
        "tmp",
        "usr",
        "usr/bin",
    ]:
        entries[d] = (stat.S_IFDIR | 0o755, b"")
    entries["dev/console"] = (stat.S_IFCHR | 0o600, b"")
    entries["gnu-df"] = (stat.S_IFREG | 0o755, gnu.read_bytes())
    script = (
        """#!/bin/busybox.static sh
/bin/busybox.static --install -s /bin
export PATH=/bin:/sbin
set -e
mount -t devtmpfs devtmpfs /dev
mount -t proc proc /proc
mount -t sysfs sysfs /sys
exec >/dev/console 2>&1
trap 'dmesg | tail -40; echo ORACLE_FAILURE; poweroff -f' EXIT
"""
        + "\n".join(commands)
        + """
for i in 1 2 3 4; do test -b /dev/vda2 && break; sleep 1; done
mount -t btrfs -o rw /dev/vda2 /mnt/btrfs
mount -t vfat -o rw,iocharset=iso8859-1 /dev/vda1 /mnt/fat
export LC_ALL=C TZ=UTC
/gnu-df --version | head -1
for type in btrfs fat; do
 stat -f -c "STATVFS $type %S %b %f %a" /mnt/$type
 echo DF_BEGIN_$type
 /gnu-df -B1 --output=size,used,avail,pcent /mnt/$type
 echo DF_END
done
umount /mnt/btrfs
umount /mnt/fat
sync
btrfs inspect-internal dump-super /dev/vda2 | grep -E "^(bytes_used|generation|total_bytes)"
echo ORACLE_OK
trap - EXIT
poweroff -f
"""
    )
    entries["init"] = (stat.S_IFREG | 0o755, script.encode())
    initrd = work / "oracle-initrd.gz"
    linux.newc(entries, initrd)
    # Both systems mount read/write, from byte-identical input. Linux may
    # commit a new generation on unmount; keep its entire disk on this RAM volume.
    linux_disk = work / "linux-copy.img"
    shutil.copyfile(disk, linux_disk)
    cmd = [
        "qemu-system-aarch64",
        "-machine",
        "virt",
        "-cpu",
        "cortex-a57",
        "-m",
        "384",
        "-nic",
        "none",
        "-nographic",
        "-no-reboot",
        "-kernel",
        kernel,
        "-initrd",
        initrd,
        "-append",
        "console=ttyAMA0 loglevel=3 panic=-1 rdinit=/init",
        "-drive",
        f"if=none,id=root,format=raw,file={linux_disk},readonly=off",
        "-device",
        "virtio-blk-device,drive=root",
    ]
    result = subprocess.run(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=120
    )
    log.write_bytes(result.stdout)
    if (
        result.returncode
        or b"ORACLE_OK" not in result.stdout
        or b"df (GNU coreutils) 9.12" not in result.stdout
    ):
        raise RuntimeError(f"Linux GNU df failed: {log}")
    if warm:
        shutil.copyfile(linux_disk, disk)
    linux_disk.unlink()
    return {
        key: parse(
            result.stdout.split(f"DF_BEGIN_{key}\r\n".encode())[-1].split(b"DF_END")[0]
        )
        for key in ["btrfs", "fat"]
    }


def parse(data):
    match = re.search(rb"(?m)^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)%\s*$", data)
    if not match:
        raise ValueError(f"df field row missing: {data!r}")
    return list(map(int, match.groups()))


def verify(arch, args):
    mode = "bios" if arch == "i686" else "uefi"
    args.proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as temp:
        work = Path(temp)
        seed = work / "seed"
        seed.mkdir()
        (seed / "dataset").write_bytes(bytes(range(256)) * 128)
        root = work / "root.img"
        subprocess.run(
            [
                sys.executable,
                linux.ROOT / "scripts/linux-btrfs.py",
                "create",
                "--root",
                seed,
                "--image",
                root,
                "--cache",
                args.cache,
            ],
            check=True,
        )
        fat = work / "fat.img"
        with fat.open("xb") as f:
            f.truncate(4 * capacity.MIB)
        subprocess.run(["mformat", "-N", "0x12345678", "-i", fat, "::"], check=True)
        subprocess.run(
            [
                sys.executable,
                linux.ROOT / "scripts/build-uutils.py",
                "--arch",
                arch,
                "--archive",
                args.archive,
                "--output",
                work,
                "--offline",
                "df",
            ],
            check=True,
        )
        subprocess.run(["mcopy", "-i", fat, work / "df", "::df.elf"], check=True)
        probe = work / "mnt.elf"
        subprocess.run(
            [
                sys.executable,
                linux.ROOT / "ports/musl/cc.py",
                "--arch",
                arch,
                "-o",
                probe,
                linux.ROOT / "tests/native-df/mounts.c",
            ],
            check=True,
        )
        subprocess.run(["mcopy", "-i", fat, probe, "::mnt.elf"], check=True)
        disk = work / "hybrid.img"
        size = root.stat().st_size
        with disk.open("xb") as f:
            f.truncate(capacity.ROOT + size + capacity.MIB)
            for pos, file in [(capacity.MIB, fat), (capacity.ROOT, root)]:
                f.seek(pos)
                with file.open("rb") as src:
                    shutil.copyfileobj(src, f)
        root.unlink()
        capacity.partition(disk, size, False)
        oracle(
            disk,
            work,
            args.gnu_df,
            args.cache,
            args.proof / (arch + "-warm.serial"),
            warm=True,
        )
        image_hash = sha(disk)
        reference = oracle(
            disk, work, args.gnu_df, args.cache, args.proof / (arch + "-linux.serial")
        )
        kernel = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
        serial = args.proof / (arch + ".serial")
        vm = Shell(
            qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
            + ["-drive", f"if=ide,index=0,format=raw,file={disk}"],
            serial,
            work,
        )
        try:
            vm.wait(b"A:\\> ")
            vm.command("c:")
            vm.command("mnt.elf", expect=b"MOUNT_ABI_OK")
            values = {}
            # The native DOS front end passes the arguments literally; feed
            # punctuation through the QEMU monitor key names.
            import qemu_shell

            qemu_shell.KEYS.update({"=": "equal", ",": "comma"})
            for kind, path in [("btrfs", "/"), ("fat", "/dos/c")]:
                start = serial.stat().st_size
                vm.command(f"df.elf -B1 --output=size,used,avail,pcent {path}")
                values[kind] = parse(serial.read_bytes()[start:])
                vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
            start = serial.stat().st_size
            vm.command("df.elf -T")
            listed = serial.read_bytes()[start:]
            if not all(
                x in listed for x in [b"UUID=", b"btrfs", b"/dos/c", b"vfat", b"/dos/a"]
            ):
                raise RuntimeError(f"missing mounted volume in df -T: {listed!r}")
        finally:
            vm.close()
        if sha(disk) != image_hash:
            raise RuntimeError("A read-only query changed the underlying image")
        report = {
            "arch": arch,
            "firmware": mode,
            "guest": values,
            "gnu_linux": reference,
            "disk_sha256": image_hash,
            "kernel_sha256": sha(kernel),
            "elf_sha256": sha(work / "df"),
            "gnu_df_sha256": sha(args.gnu_df),
        }
        (args.proof / (arch + ".json")).write_text(json.dumps(report, indent=2) + "\n")
        raw = {}
        for side, log in [
            ("dosrs", serial),
            ("linux", args.proof / (arch + "-linux.serial")),
        ]:
            fields = {}
            for name, unit, blocks, free, avail in re.findall(
                rb"STATVFS (btrfs|fat) (\d+) (\d+) (\d+) (\d+)", log.read_bytes()
            ):
                unit, blocks, free, avail = map(int, (unit, blocks, free, avail))
                used = blocks - free
                denom = used + avail
                fields[name.decode()] = [
                    blocks * unit,
                    used * unit,
                    avail * unit,
                    (100 * used + denom - 1) // denom if denom else 0,
                ]
            raw[side] = fields
        report["statvfs_derived"] = raw
        (args.proof / (arch + ".json")).write_text(json.dumps(report, indent=2) + "\n")
        if values != raw["dosrs"] or reference != raw["linux"]:
            raise RuntimeError(f"{arch}: df disagrees with raw statvfs: {report}")
        if (
            values["fat"] != reference["fat"]
            or values["btrfs"][0] != reference["btrfs"][0]
        ):
            raise RuntimeError(
                f"{arch}: on-disk capacity differs across kernels: {report}"
            )
        print(
            f"{arch}: df and GNU 9.12 match their statvfs, FAT fields and Btrfs total size verified",
            flush=True,
        )


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--ram-dir", required=True, type=Path)
    p.add_argument("--archive", required=True, type=Path)
    p.add_argument(
        "--gnu-df",
        required=True,
        type=Path,
        help="static aarch64-linux GNU df 9.12 ELF",
    )
    p.add_argument("--arch", action="append", choices=qemu.ARCH)
    p.add_argument("--cache", type=Path, default=qemu.ROOT / "target/vfs-linux-tools")
    p.add_argument("--proof", type=Path, default=qemu.ROOT / "target/native-df-proof")
    args = p.parse_args()
    for arch in args.arch or qemu.ARCH:
        verify(arch, args)
