#!/usr/bin/env python3
"""Create or independently check a raw Btrfs root with Linux btrfs-progs in RAM."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BOOT = "https://dl-cdn.alpinelinux.org/alpine/v3.22/releases/aarch64/netboot/"
BOOT_HASHES = {"vmlinuz-virt": "ebf299aa6829b15166008db24b076ac0a8b3e778c72ed66be488742aaf05d741",
               "initramfs-virt": "0f7da92ca19e6b87b8f0a250e3ce84cc8335c16b2369b18d96f68cc486d638b8"}
REPO = "https://dl-cdn.alpinelinux.org/alpine/v3.22/main/aarch64/"


def download(url, name, digest, cache):
    file = cache / name
    if not file.exists():
        with urllib.request.urlopen(url + name, timeout=45) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"bad checksum for {name}")
        file.write_bytes(data)
    if hashlib.sha256(file.read_bytes()).hexdigest() != digest:
        raise ValueError(f"bad checksum for {file}")
    return file


def packages(cache):
    entries = {}
    cache.mkdir(parents=True, exist_ok=True)
    for line in (ROOT / "scripts/btrfs-tools.sha256").read_text().splitlines():
        digest, filename = line.split()
        archive = download(REPO, filename, digest, cache)
        with tarfile.open(archive, mode="r:gz", ignore_zeros=True) as tar:
            for m in tar:
                if m.name.startswith("."):
                    continue
                path = m.name.rstrip("/")
                if m.isdir():
                    entries[path] = (stat.S_IFDIR | m.mode, b"")
                elif m.issym():
                    entries[path] = (stat.S_IFLNK | m.mode, os.fsencode(m.linkname))
                elif m.isfile():
                    entries[path] = (stat.S_IFREG | m.mode, tar.extractfile(m).read())
    return entries


def newc(entries, path):
    # Linux initramfs accepts SVR4 newc without requiring host root or mknod.
    with gzip.open(path, "wb", compresslevel=2) as out:
        for ino, (name, (mode, data)) in enumerate([*sorted(entries.items()), ("TRAILER!!!", (0,b""))], 1):
            name = os.fsencode(name) + b"\0"
            fields = (ino, mode, 0, 0, 2 if stat.S_ISDIR(mode) else 1, 0, len(data),
                      0, 0, 5 if stat.S_ISCHR(mode) else 0, 1 if stat.S_ISCHR(mode) else 0, len(name), 0)
            header = b"070701" + b"".join(f"{v:08x}".encode() for v in fields)
            out.write(header + name + bytes(-(len(header)+len(name)) % 4))
            out.write(data + bytes(-len(data) % 4))


def unpack(tar_path, output):
    # Treat all names as guest data. Materialize symbolic links last and never
    # write through an existing symlink. Keep Linux ownership in the manifest.
    if any(output.iterdir()):
        raise ValueError("restore output must be empty")
    def relative(name):
        p = Path(name)
        if p.is_absolute() or ".." in p.parts:
            raise ValueError(f"unsafe archive path {name!r}")
        return p
    with tarfile.open(tar_path) as archive:
        records = archive.getmembers()
        names = set()
        manifest = []
        for member in records:
            dest = relative(member.name)
            if dest in names:
                raise ValueError(f"duplicate archive path {dest}")
            names.add(dest)
            manifest.append(dict(path=str(dest), size=member.size, mode=member.mode,
                                 uid=member.uid, gid=member.gid, mtime=int(member.mtime),
                                 target=member.linkname))
        for m in records:
            target = output / relative(m.name)
            if m.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif m.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("xb") as dest:
                    shutil.copyfileobj(archive.extractfile(m), dest)
                target.chmod(m.mode & 0o777)
        for m in records:
            if m.islnk():
                origin = output / relative(m.linkname)
                if not origin.is_file() or origin.is_symlink():
                    raise ValueError(f"invalid hard link {m.linkname!r}")
                os.link(origin, output / relative(m.name))
        symbolic = {relative(m.name) for m in records if m.issym()}
        for m in records:
            name = relative(m.name)
            if m.issym():
                if any(parent in symbolic for parent in name.parents):
                    raise ValueError(f"symlink parent for {name}")
                (output / name).symlink_to(m.linkname)
    (output / "archive.json").write_text(json.dumps(manifest, indent=2) + "\n")


def run(args, disk):
    entries = packages(args.cache)
    kernel = args.kernel or download(BOOT, "vmlinuz-virt", BOOT_HASHES["vmlinuz-virt"], args.cache)
    modules = args.modules or download(BOOT, "initramfs-virt", BOOT_HASHES["initramfs-virt"], args.cache)
    for directory in ("dev","proc","sys","seed","out","captured","tmp"):
        entries[directory] = (stat.S_IFDIR | 0o755,b"")
    entries["dev/console"] = (stat.S_IFCHR | 0o600,b"")
    script = """#!/bin/busybox.static sh
/bin/busybox.static --install -s /bin
export PATH=/bin:/sbin
mount -t devtmpfs devtmpfs /dev
mount -t proc proc /proc
mount -t sysfs sysfs /sys
exec >/dev/console 2>&1
trap 'echo VFS_HELPER_ERROR; poweroff -f' EXIT
set -ex
for mod in /lib/modules/*/kernel/drivers/virtio/virtio_mmio.ko /lib/modules/*/kernel/drivers/block/virtio_blk.ko; do
  if test -f "$mod"; then /bin/busybox.static insmod "$mod"; fi
done
for n in 1 2 3 4 5; do test -b /dev/vda && break; sleep 1; done
test -b /dev/vda
for device in /sys/block/vd*; do
  serial=$(cat "$device/serial")
  case "$serial" in
    DOSRSROOT) disk=/dev/${device##*/} ;;
    DOSRSCHECK) capture=/dev/${device##*/} ;;
  esac
done
test -n "$disk"
"""
    if args.command == "create":
        # No existing physical or virtual disk is overwritten.
        with disk.open("xb") as out:
            out.truncate(args.mib << 20)
        for path in sorted(args.root.rglob("*")):
            name = "seed/" + str(path.relative_to(args.root))
            mode = path.lstat().st_mode
            data = os.fsencode(os.readlink(path)) if path.is_symlink() else path.read_bytes() if path.is_file() else b""
            entries[name] = (mode, data)
        script += f'mkfs.btrfs -f -n {args.nodesize} -m single -d single -L DOSRSROOT -r /seed "$disk"\n'
    else:
        if not disk.is_file():
            raise FileNotFoundError(disk)
        args.output.mkdir(parents=True,exist_ok=True)
        script += """btrfs check --readonly --check-data-csum "$disk" >/captured/check.log 2>&1 || { cat /captured/check.log; exit 1; }
btrfs inspect-internal dump-tree -t 5 "$disk" >/captured/tree.txt
btrfs inspect-internal dump-tree -t chunk "$disk" >/captured/chunks.txt
mkdir /captured/files
btrfs restore -S -m "$disk" /captured/files
tar cf "$capture" -C /captured .
"""
    script += "sync\necho VFS_HELPER_OK\ntrap - EXIT\npoweroff -f\n"
    entries["init"] = (stat.S_IFREG | 0o755,script.encode())
    with tempfile.TemporaryDirectory(dir=disk.parent) as temp:
        tmp = Path(temp)
        initrd = tmp / "initrd.gz"
        custom = tmp / "custom.gz"
        newc(entries, custom)
        with initrd.open("wb") as out:
            for part in (modules, custom):
                with part.open("rb") as segment:
                    shutil.copyfileobj(segment, out)
        cmd = ["qemu-system-aarch64","-machine","virt","-cpu","cortex-a57","-m","384","-nic","none",
               "-nographic","-no-reboot","-kernel",kernel,"-initrd",initrd,
               "-append","console=ttyAMA0 loglevel=3 panic=-1 rdinit=/init",
               "-drive",f"if=none,id=root,format=raw,file={disk},readonly={'off' if args.command=='create' else 'on'}",
               "-device","virtio-blk-device,drive=root,serial=DOSRSROOT"]
        if args.command == "check":
            with (tmp/"capture.tar").open("xb") as out:
                out.truncate(disk.stat().st_size + (16 << 20))
            cmd += ["-drive",f"if=none,id=output,format=raw,file={tmp/'capture.tar'}","-device","virtio-blk-device,drive=output,serial=DOSRSCHECK"]
        done = subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
        sys.stdout.buffer.write(done.stdout)
        if done.returncode or b"VFS_HELPER_OK" not in done.stdout.splitlines() and b"\nVFS_HELPER_OK\r\n" not in done.stdout:
            raise RuntimeError("Linux btrfs-progs did not complete")
        if args.command == "check":
            unpack(tmp / "capture.tar", args.output)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("command",choices=["create","check"])
    cli.add_argument("--image",required=True,type=Path)
    cli.add_argument("--root",type=Path)
    cli.add_argument("--output",type=Path)
    cli.add_argument("--kernel",type=Path)
    cli.add_argument("--modules",type=Path)
    cli.add_argument("--cache",type=Path,default=ROOT/"target/vfs-linux-tools")
    cli.add_argument("--mib",type=int,default=128)
    cli.add_argument("--nodesize", type=int, choices=[4096, 8192, 16384, 32768, 65536], default=16384)
    args = cli.parse_args()
    if args.command == "create" and not (args.root and args.root.is_dir()):
        cli.error("create needs an existing --root directory")
    if args.command == "check" and not args.output:
        cli.error("check needs --output")
    image=args.image.resolve()
    if args.command=="create":
        if image.exists():
            raise FileExistsError(image)
        with tempfile.TemporaryDirectory(dir=image.parent) as staging:
            raw=Path(staging)/"complete.img"
            run(args,raw)
            os.link(raw,image)
    else:
        run(args,image)
