#!/usr/bin/env python3
"""Run POSIX file/metadata/link checks on a Btrfs root alongside a DOS FAT C:."""
import argparse
import json
import re
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time

import qemu
from qemu_shell import Shell

MIB = 1 << 20
BTRFS_START = 8 * MIB


def layout(disk, fat, root):
    start = MIB
    with disk.open("xb") as output:
        table = bytearray(512)
        for i, (kind, pos, volume) in enumerate(((0x01, start, fat), (0x83, BTRFS_START, root))):
            struct.pack_into("<B3xB3xII",table,446+16*i,0,kind,pos//512,volume.stat().st_size//512)
        table[510:] = b"\x55\xaa"
        output.write(table)
        for pos, volume in [(start,fat),(BTRFS_START,root)]:
            output.seek(pos)
            with volume.open("rb") as input:
                shutil.copyfileobj(input, output)


def helper(command, *args):
    subprocess.run([sys.executable,qemu.ROOT / "scripts/linux-btrfs.py",command,*args],check=True)


def verify(root, output):
    helper("check","--image",root,"--output",output)
    if (output/"files/at-hard").read_bytes()!=b"abc\x00":
        raise RuntimeError("Linux restored incorrect truncated data")
    if (output/"files/attrs/file").read_bytes()!=b"aXc" or (output/"files/attrs/sym").readlink()!=Path("file"):
        raise RuntimeError("Linux restored incorrect positioned write or symlink")
    files = output / "files/work"
    if (files/"result").read_bytes()!=b"newdata" or (files/"alias").read_bytes()!=b"newdata" or (files/"dst").read_bytes()!=b"swap":
        raise RuntimeError("Linux restored different file bytes")
    if (files/"view").readlink()!=Path("alias") or (files/"loop").exists() or (files/"moved").exists():
        raise RuntimeError("wrong restored link or directory")
    meta = {entry["path"]:entry for entry in json.loads((output/"archive.json").read_text())}
    r = meta["files/work/result"]
    if (r["mode"], r["uid"], r["gid"]) != (0o640,123,456):
        raise RuntimeError("Linux restored different mode/uid/gid")
    dump = (output/"tree.txt").read_text()
    if not all(stamp in dump for stamp in ["mtime 42.123456789", "mtime 4567890123.456"]):
        raise RuntimeError("Linux found incorrect persistent 64-bit file or symlink timestamps")
    refs=re.findall(r"location key \((\d+) INODE_ITEM 0\) type FILE\n[^\n]*\n\s*name: (alias|result)\n",dump)
    if {name for _,name in refs}!={"alias","result"} or len({ino for ino,_ in refs})!=1:
        raise RuntimeError("Linux did not find both names referencing the same inode")
    if "mode 100640 links 2 uid 123 gid 456" not in dump:
        raise RuntimeError("Linux found the wrong hard link count or permissions")
    if "ORPHAN_ITEM" in dump:
        raise RuntimeError("cold boot did not reclaim orphan inode")
    if "no error found" not in (output/"check.log").read_text():
        raise RuntimeError("Linux btrfs-progs check did not pass")


def command(vm, line, expected):
    begin=vm.log.stat().st_size
    vm.enter(line)
    deadline=time.monotonic()+300
    while time.monotonic()<deadline and vm.vm.poll() is None:
        serial=vm.log.read_bytes()[begin:]
        if b"C:\\> " in serial:
            if expected not in serial: raise RuntimeError(serial.decode(errors="replace"))
            return
        time.sleep(0.2)
    raise RuntimeError(f"incomplete native VFS probe: {line}")


def run(arch, ram, proof, prepared, musl):
    kernel=qemu.build(arch,qemu.ROOT/"target")
    with tempfile.TemporaryDirectory(dir=ram) as tmp:
        work=Path(tmp)
        root=work/"root.img"
        if prepared:
            shutil.copyfile(prepared,root)
        else:
            tree=work/"tree";tree.mkdir()
            if musl:
                (tree/"bin").mkdir()
                for name,source in [("fstest","files"),("locktest","locks")]:
                    elf=tree/"bin"/(name+".elf")
                    subprocess.run([sys.executable,qemu.ROOT/"ports/musl/cc.py","--arch",arch,
                                    "-o",elf,qemu.ROOT/"ports/musl/tests"/(source+".c")],check=True)
                    elf.chmod(0o755)
            helper("create","--root",tree,"--image",root)
        fat=work/"fat.img"
        with fat.open("xb") as out: out.truncate(4*MIB)
        subprocess.run(["mformat","-i",fat,"::"],check=True)
        for name,source in [("vfsc","vfscheck"),("atcheck","atcheck"),("attrck","attrcheck"),("lockck","lockcheck")]:
            elf=work/(name+".elf")
            subprocess.run([sys.executable,qemu.ROOT/"scripts/native-cc.py","--arch",arch,"-o",elf,qemu.ROOT/"native-exec/examples"/(source+".c")],check=True)
            subprocess.run(["mcopy","-i",fat,elf,"::"],check=True)
        disk=work/"data.img";layout(disk,fat,root)
        for phase in ("first","cold"):
            vm=Shell(qemu.boot_args(kernel,arch,work/"boot",4,"bios" if arch=="i686" else "uefi",esp_mib=36)
                     + ["-drive",f"if=ide,index=0,format=raw,file={disk}"],proof/f"{arch}-{phase}.serial",work)
            try:
                vm.wait(b"Mounted Btrfs / (read/write).")
                vm.wait(b"Mounted persistent FAT volume C:")
                vm.wait(b"A:\\> ")
                vm.command("c:")
                if phase=="first" and musl:
                    command(vm,"/bin/fstest.elf",b"musl Btrfs directory enumeration permissions links and unlink OK")
                    command(vm,"/bin/locktest.elf",b"musl fcntl process locks and 64-bit ranges OK")
                if phase=="first":
                    command(vm,"lockck.elf",b"advisory lock ranges ownership and wakeup OK")
                    command(vm,"atcheck.elf",b"directory fd and file flags OK")
                    command(vm,"attrck.elf",b"file times and positioned I/O OK")
                command(vm,"vfsc.elf" if phase=="first" else "vfsc.elf cold",
                        b"Btrfs root VFS OK" if phase=="first" else b"cold root OK")
            finally:
                vm.close()
        with disk.open("rb") as full, root.open("wb") as out:
            full.seek(BTRFS_START);shutil.copyfileobj(full,out)
        shared=subprocess.check_output(["mcopy","-i",f"{disk}@@{MIB}","::DOS.TXT","-"])
        if shared!=b"shared":raise RuntimeError("wrong stopped DOS FAT contents")
        # Discard the temporary combined disk before provisioning the recovery VM.
        disk.unlink()
        verify(root,proof/arch)
        print(arch,"cold native ELF, FAT C:, Linux Btrfs checks and restored metadata passed",flush=True)


if __name__=="__main__":
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch",choices=["i686","x86_64"],action="append")
    cli.add_argument("--ram-dir",type=Path,required=True)
    cli.add_argument("--proof",type=Path,required=True)
    cli.add_argument("--musl",action="store_true",help="build and run both public libc probes from /bin using the installed musl sysroot")
    cli.add_argument("--root-image",type=Path,help="use a copy of an existing disposable root fixture")
    a=cli.parse_args();a.proof.mkdir(parents=True,exist_ok=True)
    for arch in a.arch or ["i686","x86_64"]:
        run(arch,a.ram_dir.resolve(),a.proof.resolve(),a.root_image,a.musl)
