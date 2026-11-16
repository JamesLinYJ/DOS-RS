#!/usr/bin/env python3
"""Exercise Btrfs on a disposable raw image, including a cold boot with an open orphan."""
import argparse
import gzip
import hashlib
import io
import subprocess
import tarfile
from pathlib import Path
import shutil
import tempfile

import qemu
from qemu_shell import Shell

def run(arch, firmware, work, disk, proof, cold):
    kernel = qemu.build(arch, qemu.ROOT / "target")
    cmd = qemu.boot_args(kernel, arch, work / "boot", 4, firmware)
    cmd += ["-drive", f"if=ide,index=0,format=raw,file={disk}"]
    vm = Shell(cmd, proof / f"{arch}-{'cold' if cold else 'first'}.log", work)
    try:
        vm.wait(b"Mounted Btrfs C:\\ (read/write).")
        vm.wait(b"A:\\> ")
        vm.command("c:")
        if cold:
            vm.command("type count.txt", expect=b"\r\n20480\n")
            vm.command("cat pending.tmp", expect=b"no such file or directory")
            return
        vm.command('cat "long sample.bin" | wc -c > count.txt')
        vm.command("cat count.txt", expect=b"\r\n20480\r\n")
        vm.command("fdcheck verify.tmp", expect=b"old fd: original!; new file: replacement")
        vm.command("cd nested", prompt=b"C:\\nested> ")
        vm.command("cat name.txt", prompt=b"C:\\nested> ", expect=b"from nested directory")
        vm.command("cd ..")
        vm.command("hello.com", expect=b"GUEST HELLO COM")
        vm.command("cat note.txt > pending.tmp")
        vm.enter("cat >> pending.tmp")
        vm.key("alt-f2")
        vm.wait(b"DOS terminal 2")
        vm.command("c:")
        vm.command("ren pending.tmp orphan.tmp", expect=b"File renamed.")
        vm.command("del orphan.tmp", expect=b"File deleted.")
        vm.command("cat pending.tmp", expect=b"no such file or directory")
        # Stop the machine with the first terminal's descriptor still open.
    finally:
        vm.close()

CHECK = """set -eu
cat > /qa/disk
btrfs check --readonly --check-data-csum /qa/disk
mkdir /qa/files
btrfs restore /qa/disk /qa/files
test "$(cat /qa/files/count.txt)" = 20480
test ! -e /qa/files/orphan.tmp
test ! -e /qa/files/pending.tmp
test ! -e /qa/files/verify.tmp
sha256sum '/qa/files/long sample.bin'
btrfs inspect-internal dump-tree -t 5 /qa/disk > /qa/tree
if grep -q 'ORPHAN_ITEM' /qa/tree; then exit 1; fi
"""

def linux_cmd(image, script):
    return ["docker", "run", "--rm", "--network", "none", "-i", "--tmpfs",
            "/qa:rw,size=200m", image, "sh", "-ec", script]

def fixture(image, work, disk):
    payload = bytes(range(256)) * 80
    hello = subprocess.check_output(["mtype", "-i", qemu.ROOT / "disks/guest12.img", "::hello.com"])
    root = work / "root"
    (root / "nested").mkdir(parents=True)
    files = {"hello.com": hello, "long sample.bin": payload, "note.txt": b"DOS-RS Btrfs\n",
             "nested/name.txt": b"from nested directory\n"}
    for name, contents in files.items():
        (root / name).write_bytes(contents)
    if image:
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as tar:
            tar.add(root, arcname=".")
        script = """mkdir /qa/root
tar xf - -C /qa/root
truncate -s 128M /qa/disk
mkfs.btrfs -f -L DOSRS -m single -d single -r /qa/root /qa/disk >&2
cat /qa/disk"""
        with disk.open("xb") as out:
            subprocess.run(linux_cmd(image, script), input=archive.getvalue(), stdout=out, check=True)
    else:
        with disk.open("xb") as out:
            out.truncate(128 << 20)
        subprocess.run(["mkfs.btrfs", "-f", "-L", "DOSRS", "-m", "single", "-d", "single",
                        "-r", root, disk], check=True)

def verify(image, work, disk, log):
    digest = hashlib.sha256(bytes(range(256)) * 80).hexdigest()
    if image:
        with disk.open("rb") as data, log.open("wb") as out:
            subprocess.run(linux_cmd(image, CHECK), stdin=data, stdout=out,
                           stderr=subprocess.STDOUT, check=True)
        if digest not in log.read_text():
            raise ValueError("Linux recovered different payload bytes")
        return
    restored = work / "restored"
    restored.mkdir()
    with log.open("wb") as out:
        subprocess.run(["btrfs","check","--readonly","--check-data-csum",disk],
                       stdout=out,stderr=subprocess.STDOUT,check=True)
    subprocess.run(["btrfs","restore",disk,restored],check=True)
    if (restored/"count.txt").read_bytes() != b"20480\n" or hashlib.sha256(
            (restored/"long sample.bin").read_bytes()).hexdigest() != digest:
        raise ValueError("Linux recovered different file bytes")
    if any((restored/n).exists() for n in ["orphan.tmp","pending.tmp","verify.tmp"]):
        raise ValueError("deleted file reappeared")
    tree = subprocess.check_output(["btrfs","inspect-internal","dump-tree","-t","5",disk])
    if b"ORPHAN_ITEM" in tree:
        raise ValueError("orphan survived cold boot")

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--fixture", type=Path, help="optional gzip-compressed copy of the test volume")
    p.add_argument("--ram-dir", type=Path, required=True)
    p.add_argument("--proof", type=Path, default=qemu.ROOT / "target/btrfs-proof")
    p.add_argument("--linux-tools-image", help="local Linux container containing btrfs-progs")
    p.add_argument("--arch", choices=list(qemu.ARCH))
    args = p.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in [args.arch] if args.arch else qemu.ARCH:
        firmware = "bios" if arch == "i686" else "uefi"
        with tempfile.TemporaryDirectory(dir=args.ram_dir) as root:
            work = Path(root)
            disk = work / "c.img"
            if args.fixture:
                with gzip.open(args.fixture, "rb") as src, disk.open("xb") as dst:
                    shutil.copyfileobj(src, dst)
            else:
                fixture(args.linux_tools_image, work, disk)
            for cold in (False, True):
                run(arch, firmware, work, disk, args.proof, cold)
            verify(args.linux_tools_image,work,disk,args.proof/f"{arch}-linux.log")
        print(f"{arch}/{firmware}: cold boot, orphan cleanup, Linux checksum and data readback passed", flush=True)

if __name__ == "__main__":
    main()
