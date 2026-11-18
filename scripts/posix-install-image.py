#!/usr/bin/env python3
"""Create a new bootable DOS-RS disk with a DOS FAT C: and a separate POSIX Btrfs root."""
import argparse
import errno
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

import qemu

MIB = 1 << 20
SECTOR = 512


def make_root(arch, work, extra, programs=None):
    tree = work / "root"
    if extra:
        shutil.copytree(extra, tree, symlinks=True)
    for name in ("bin", "dev", "etc", "tmp", "usr", "home"):
        (tree / name).mkdir(parents=True, exist_ok=True)
    (tree / "root").mkdir(mode=0o700, exist_ok=True)
    accounts = tree / "etc"
    if not accounts.is_symlink():
        for source in (qemu.ROOT / "userland/etc").iterdir():
            dest = accounts / source.name
            try:
                with dest.open("xb") as stream:
                    stream.write(source.read_bytes())
            except FileExistsError:
                continue
            dest.chmod(0o644)
    (tree/"tmp").chmod(0o1777)
    if programs is None:
        subprocess.run([sys.executable, qemu.ROOT/"scripts/build-rust-tools.py", "--arch", arch,
                        "--output", tree/"bin"], check=True)
    else:
        for name in ("sh", "cat", "wc", "echo"):
            shutil.copyfile(programs / name, tree / "bin" / name)
    for app in (tree/"bin").iterdir():
        if app.is_file(): app.chmod(0o755)
    volume = work / "root.img"
    if not shutil.which("mkfs.btrfs"):
        subprocess.run([sys.executable, qemu.ROOT / "scripts/linux-btrfs.py", "create",
                        "--root", tree, "--image", volume], check=True)
        return volume
    with volume.open("xb") as f:
        f.truncate(128*MIB)
    subprocess.run(["mkfs.btrfs", "-f", "-m", "single", "-d", "single", "-L", "DOSRS-ROOT",
                    "--rootdir", tree, volume], check=True)
    return volume


def fat(work, name, size, label, kernel=None, arch=None):
    image=work/name
    with image.open("xb") as f: f.truncate(size*MIB)
    subprocess.run(["mformat", "-F", "-v", label, "-i", image, "::"], check=True)
    if kernel:
        subprocess.run(["mmd","-i",image,"::EFI","::EFI/BOOT"],check=True)
        subprocess.run(["mcopy","-i",image,kernel,"::EFI/BOOT/"+qemu.ARCH[arch][1]],check=True)
    return image


def assemble(arch, output, ram_dir, volume=None, tree=None, trace=False, *, kernel=None, programs=None):
    if output.exists():
        raise FileExistsError(output)
    if volume:
        with volume.open("rb") as f:
            f.seek(0x10040)
            if f.read(8)!=b"_BHRfS_M":
                raise ValueError("expected a raw Btrfs volume")
    with tempfile.TemporaryDirectory(dir=ram_dir) as temporary:
        work=Path(temporary)
        kernel=kernel or qemu.build(arch,qemu.ROOT/"target",features="exec-trace" if trace else None)
        root=volume or make_root(arch,work,tree,programs)
        c=fat(work,"shared.img",36,"DOS-SHARED")
        disk=work/"system.img"
        if arch=="i686":
            subprocess.run([sys.executable,qemu.ROOT/"scripts/bios-image.py",kernel,disk],check=True)
            idx=2
            partitions=[(0x0c,c),(0x83,root)]
        else:
            esp=fat(work,"esp.img",36,"DOS-EFI",kernel,arch)
            with disk.open("xb") as f: f.write(bytes(510)+b"\x55\xaa")
            idx=0
            partitions=[(0xef,esp),(0x0c,c),(0x83,root)]
        with disk.open("r+b") as d:
            mbr=bytearray(d.read(SECTOR))
            d.seek(0,2)
            for number,(kind,source) in enumerate(partitions,idx):
                start=(d.tell()+MIB-1)//MIB*MIB
                length=source.stat().st_size
                if length%SECTOR:
                    raise ValueError("unaligned partition size")
                chs=b"\xfe\xff\xff"
                struct.pack_into("<B3sB3sII",mbr,446+16*number,0,chs,kind,chs,start//SECTOR,length//SECTOR)
                d.seek(start)
                with source.open("rb") as src: shutil.copyfileobj(src,d)
            d.seek(0);d.write(mbr);d.flush();os.fsync(d.fileno())
        # The publish step never opens an existing user disk for writing.
        output.parent.mkdir(parents=True,exist_ok=True)
        try:
            os.link(disk, output)
            return output
        except OSError as exc:
            if exc.errno != errno.EXDEV:
                raise
        with tempfile.NamedTemporaryFile(dir=output.parent) as staging:
            with disk.open("rb") as src: shutil.copyfileobj(src,staging)
            staging.flush();os.fsync(staging.fileno());os.link(staging.name,output)
    return output


if __name__=="__main__":
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch",choices=qemu.ARCH,required=True)
    cli.add_argument("--ram-dir",type=Path,required=True)
    source=cli.add_mutually_exclusive_group()
    source.add_argument("--root-image",type=Path,help="use an already populated, unmounted Btrfs volume of the target architecture")
    source.add_argument("--root-tree",type=Path,help="additional POSIX files to include when running mkfs.btrfs")
    cli.add_argument("--output",type=Path,required=True)
    cli.add_argument("--exec-trace",action="store_true",help="include executed paths on the kernel serial console for QA")
    args=cli.parse_args()
    print(assemble(args.arch,args.output,args.ram_dir,args.root_image,args.root_tree,args.exec_trace))
