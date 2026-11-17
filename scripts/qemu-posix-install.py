#!/usr/bin/env python3
"""Cold-boot both installed partitions as a single disk and verify Btrfs with Linux."""
import argparse
import base64
import hashlib
import gzip
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


def module(name):
    spec=importlib.util.spec_from_file_location(name,qemu.ROOT/"scripts"/(name+".py"))
    m=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m

def boot(arch,disk,work):
    machine=qemu.ARCH[arch][0]
    args=["qemu-system-"+machine,"-m","384","-smp","4","-no-reboot",
          "-drive",f"if=ide,index=0,format=raw,file={disk}","-boot","c"]
    if arch=="x86_64":
        code=qemu.firmware(machine)
        template=Path(os.environ.get("DOS_RS_FIRMWARE_VARS",str(code.parent/"edk2-i386-vars.fd")))
        var=work/"vars.fd"
        shutil.copyfile(template,var)
        args+=["-drive",f"if=pflash,unit=0,format=raw,readonly=on,file={code}",
               "-drive",f"if=pflash,unit=1,format=raw,file={var}"]
    return args

def partitions(disk):
    with disk.open("rb") as f: mbr=f.read(512)
    return {mbr[450+16*i]:struct.unpack_from("<II",mbr,454+16*i)
            for i in range(4) if mbr[450+16*i]}

def extract(disk,part,target):
    at,count=part
    with disk.open("rb") as src,target.open("xb") as out:
        src.seek(at*512)
        size=count*512
        while size:
            data=src.read(min(size,1<<20))
            if not data: raise ValueError("truncated partition")
            out.write(data)
            size-=len(data)

def archive_disk(disk, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    original = hashlib.sha256()
    with tempfile.NamedTemporaryFile(dir=destination.parent) as staged:
        with disk.open("rb") as src, gzip.GzipFile(filename="", mode="wb", fileobj=staged, mtime=0) as compressed:
            for chunk in iter(lambda: src.read(1 << 20), b""):
                compressed.write(chunk)
                original.update(chunk)
        staged.flush()
        os.fsync(staged.fileno())
        restored = hashlib.sha256()
        with gzip.open(staged.name, "rb") as src:
            for chunk in iter(lambda: src.read(1 << 20), b""):
                restored.update(chunk)
        if original.digest() != restored.digest():
            raise ValueError("archived disk differs")
        os.link(staged.name, destination)
    return original.hexdigest()


def run(arch,work,proof,volume,archive):
    installer=module("posix-install-image")
    shell=module("qemu-user-shell")
    mode="bios" if arch=="i686" else "uefi"
    disk=work/"system.img"
    data=work/"tree"
    expected = {"count.txt": b"21254\n", "copy.bin": shell.DATA, "a.txt": shell.DATA}
    std_tools = volume is None and bool(os.environ.get("DOSRS_UUTILS_ARCHIVE"))
    if volume is None:
        data.mkdir()
        source = qemu.ROOT / "tests/user-shell/control.sh"
        shutil.copyfile(source, data / "control.sh")
        reference = subprocess.run(["/bin/sh", source, "one", "two words"], cwd=work,
                                   env=dict(os.environ, LC_ALL="C"), capture_output=True)
        if reference.returncode != 13 or reference.stderr:
            raise ValueError("shell fixture failed on the reference host")
        expected.update({"subst.out": reference.stdout, "subst.rc": b"13\n"})
        if std_tools:
            expected.update({"encoded.txt": base64.encodebytes(shell.DATA), "decoded.bin": shell.DATA})
        (data/"a.txt").write_bytes(shell.DATA)
    installer.assemble(arch,disk,work,volume,data if volume is None else None,trace=True)
    table=partitions(disk)
    app=work/"fdcheck.elf"
    subprocess.run([sys.executable,qemu.ROOT/"scripts/native-cc.py","--arch",arch,"-o",app,
                    qemu.ROOT/"native-exec/examples/fdcheck.c"],check=True)
    fat=table[0x0c]
    subprocess.run(["mcopy","-i",f"{disk}@@{fat[0]*512}",app,"::FDCHECK.ELF"],check=True)
    serial=proof/(arch+"-install.serial")
    shell.exercise(boot(arch,disk,work),arch,mode,work,serial,dual_volume=True,grammar=volume is None,std_tools=std_tools)
    second=Shell(boot(arch,disk,work),proof/(arch+"-cold.serial"),work)
    try:
        second.wait(b"A:\\> ")
        second.command("sh",prompt=b"$ ")
        second.command("cat count.txt",prompt=b"$ ",expect=b"21254\r\n")
        second.command("exit",prompt=b"A:\\> ")
        second.command("hello.com",prompt=b"A:\\> ",expect=b"GUEST HELLO COM")
    finally:
        second.close()
    with tempfile.TemporaryDirectory(dir=work) as temporary:
        tmp=Path(temporary)
        fat_copy=tmp/"fat.img"
        extract(disk,fat,fat_copy)
        fsck=subprocess.check_output(["fsck.fat","-n",fat_copy],text=True)
        (proof/(arch+"-fat.log")).write_text(fsck)
        # A DOS C: file must not alias a relative path on the POSIX root.
        listing=subprocess.check_output(["mdir","-b","-i",fat_copy,"::"])
        if b"fdcheck.elf" not in listing.lower() or b"count.txt" in listing.lower():
            raise ValueError("incorrect DOS volume contents")
        btrfs=tmp/"root.img"
        extract(disk,table[0x83],btrfs)
        restored=tmp/"restored"
        cmd=[sys.executable,qemu.ROOT/"scripts/linux-btrfs.py","check","--image",btrfs,"--output",restored]
        if shutil.which("btrfs"):
            restored.mkdir()
            checks=[["btrfs","check","--readonly","--check-data-csum",btrfs],["btrfs","restore",btrfs,restored]]
        else:
            checks=[cmd]
        with (proof/(arch+"-linux.log")).open("wb") as log:
            for cmd in checks:
                subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,check=True)
        if not shutil.which("btrfs"):
            shutil.copyfile(restored/"check.log",proof/(arch+"-btrfs-check.log"))
            restored=restored/"files"
        result={}
        for name,reference_bytes in expected.items():
            actual=(restored/name).read_bytes()
            if actual!=reference_bytes:
                (proof/(arch+"-"+name+".actual")).write_bytes(actual)
                (proof/(arch+"-"+name+".expected")).write_bytes(reference_bytes)
                raise ValueError(f"Linux recovered different {name}")
            result[name]={"bytes":len(actual),"sha256":hashlib.sha256(actual).hexdigest()}
            (proof/(arch+"-"+name)).write_bytes(actual)
        if (restored/"gone.tmp").exists():raise ValueError("unlinked file survived")
        (proof/(arch+".json")).write_text(json.dumps({"arch":arch,"kernel_bits":32 if arch=="i686" else 64,"firmware":mode,"files":result,
                "qemu":subprocess.check_output(["qemu-system-"+qemu.ARCH[arch][0],"--version"], text=True).splitlines()[0],
                "partitions":{hex(k):v for k,v in table.items()}},indent=2)+"\n")
    if archive:
        verified=archive_disk(disk,archive)
        (proof/(arch+"-disk.sha256")).write_text(verified+"  system.img\n")
    print(f"{arch}/{mode}: single-disk boot, native shell, unlink, cold read and Linux disk checks passed",flush=True)

if __name__=="__main__":
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch",required=True,choices=qemu.ARCH)
    cli.add_argument("--root-image",type=Path,help="Btrfs root with the target /bin and test a.txt")
    cli.add_argument("--ram-dir",type=Path,required=True)
    cli.add_argument("--proof",type=Path,default=qemu.ROOT/"target/posix-install-proof")
    cli.add_argument("--archive",type=Path,help="retain the checked stopped disk as a new gzip archive")
    args=cli.parse_args()
    args.proof.mkdir(parents=True,exist_ok=True)
    if args.archive and args.archive.exists():
        raise FileExistsError(args.archive)
    with tempfile.TemporaryDirectory(dir=args.proof, prefix="."+args.arch+"-") as directory:
        staged = Path(directory)
        try:
            with tempfile.TemporaryDirectory(dir=args.ram_dir) as work:
                run(args.arch,Path(work),staged,args.root_image,args.archive)
        except BaseException:
            failures = args.proof / "failed"
            failures.mkdir(exist_ok=True)
            shutil.copytree(staged, failures / staged.name.removeprefix("."))
            raise
        # Publish the JSON manifest after the logs and reference bytes it describes.
        for file in sorted(staged.iterdir(), key=lambda file: file.suffix == ".json"):
            os.replace(file, args.proof / file.name)
