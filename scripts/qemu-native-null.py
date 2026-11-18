#!/usr/bin/env python3
"""Verify the installed POSIX null device across a RAM-disk cold boot."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


BANNER = b"null: char device 1:3, EOF, discard, seek and unprivileged redirection passed"
DATA = b"DOS-RS null device regression\n"


def run(args, work):
    path = qemu.ROOT / "scripts/qemu-posix-install.py"
    spec = importlib.util.spec_from_file_location("posix_install", path)
    install = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(install)
    builder = install.module("posix-install-image")
    arch = args.arch
    firmware = "bios" if arch == "i686" else "uefi"
    oracle = version = None
    if args.gnu_cat:
        env = dict(os.environ, LC_ALL="C", TZ="UTC")
        version = subprocess.check_output([args.gnu_cat, "--version"], env=env).splitlines()[0]
        if version != b"cat (GNU coreutils) 9.12":
            raise ValueError("expected GNU coreutils cat 9.12")
        oracle = subprocess.run([args.gnu_cat, "/dev/null"], env=env, capture_output=True)
    tree = work / "seed"
    tree.mkdir()
    probe = tree / "cwdprobe"
    shutil.copyfile(args.probe, probe)
    probe.chmod(0o755)
    (tree / "a.txt").write_bytes(DATA)
    disk = work / "system.img"
    builder.assemble(arch, disk, work, tree=tree, trace=True,
                     kernel=args.kernel, programs=args.programs)

    for boot in ("first", "cold"):
        serial = args.proof / f"{arch}-null-{boot}.serial"
        vm = Shell(install.boot(arch, disk, work), serial, work)
        try:
            vm.wait(b"A:\\> ", timeout=120)
            vm.command("sh", prompt=b"$ ")
            vm.command("/cwdprobe --null-device", prompt=b"$ ", expect=BANNER)
            vm.command("echo $?", prompt=b"$ ", expect=b"0\r\n")
            if boot == "first":
                vm.command("cat a.txt | wc -c > count.txt", prompt=b"$ ", timeout=90)
                if oracle is not None:
                    vm.command("cat /dev/null > null.out 2> null.err", prompt=b"$ ")
                    vm.command("echo $? > null.rc", prompt=b"$ ")
            vm.command("cat count.txt", prompt=b"$ ", expect=f"{len(DATA)}\r\n".encode())
        finally:
            vm.close()
        qemu.check_native_boot(serial, arch, firmware)
        if b"path=/cwdprobe" not in serial.read_bytes():
            raise ValueError("the expected Rust ELF did not execute")

    table = install.partitions(disk)
    fat = work / "fat.img"
    install.extract(disk, table[0x0c], fat)
    fsck = shutil.which("fsck.fat") or "/opt/homebrew/opt/dosfstools/sbin/fsck.fat"
    (args.proof / f"{arch}-null-fat.log").write_bytes(subprocess.check_output([fsck, "-n", fat]))
    root = work / "root-check.img"
    install.extract(disk, table[0x83], root)
    fat.unlink()
    restored = work / "restored"
    with (args.proof / f"{arch}-null-linux.log").open("wb") as log:
        subprocess.run([sys.executable, qemu.ROOT / "scripts/linux-btrfs.py", "check",
                        "--image", root, "--output", restored],
                       check=True, stdout=log, stderr=subprocess.STDOUT)
    files = restored / "files"
    if (files / "count.txt").read_bytes() != f"{len(DATA)}\n".encode():
        raise ValueError("unexpected file bytes after stopping the VM")
    if oracle is not None and version is not None:
        for name, expected in (
            ("null.out", oracle.stdout),
            ("null.err", oracle.stderr),
            ("null.rc", f"{oracle.returncode}\n".encode()),
        ):
            if (files / name).read_bytes() != expected:
                raise ValueError(f"{name} differs from GNU cat")
            shutil.copyfile(files / name, args.proof / f"{arch}-{name}")
    shutil.copyfile(restored / "check.log", args.proof / f"{arch}-null-btrfs.log")
    info = {
        "architecture": arch, "kernel_bits": 32 if arch == "i686" else 64,
        "firmware": firmware, "device": [1, 3], "boots": 2,
        "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
        "kernel_sha256": hashlib.sha256(args.kernel.read_bytes()).hexdigest(),
        "stopped_count": len(DATA), "fat_check": True, "btrfs_check": True,
        "gnu_cat": version.decode() if version else None,
    }
    if args.archive:
        info["system_disk_sha256"] = install.archive_disk(disk, args.archive)
    (args.proof / f"{arch}-null.json").write_text(json.dumps(info, indent=2) + "\n")
    print(f"{arch} {firmware}: native null device and both stopped filesystems passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=qemu.ARCH, required=True)
    for name in ("kernel", "programs", "probe", "ram-dir", "proof"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--gnu-cat", type=Path, help="optional GNU coreutils 9.12 oracle")
    parser.add_argument("--archive", type=Path, help="save the verified stopped disk as a gzip archive")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="null-" + args.arch + "-", dir=args.ram_dir))
    try:
        run(args, work)
    except BaseException:
        print(f"{args.arch}: failed RAM fixture retained at {work}", flush=True)
        raise
    else:
        shutil.rmtree(work)
