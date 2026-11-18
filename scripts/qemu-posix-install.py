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


def run(arch,work,proof,volume,archive,kernel=None,programs=None,probes=None,*,shell_sweep=True):
    installer=module("posix-install-image")
    shell=module("qemu-user-shell")
    shell_sweep = volume is None and shell_sweep
    mode="bios" if arch=="i686" else "uefi"
    disk=work/"system.img"
    data=work/"tree"
    inputs = {}
    if kernel:
        snapshot = work / "kernel.efi"
        shutil.copyfile(kernel, snapshot)
        kernel = snapshot
        inputs["kernel"] = hashlib.sha256(kernel.read_bytes()).hexdigest()
    if programs:
        snapshot = work / "programs"
        snapshot.mkdir()
        for name in installer.replay_programs(programs):
            shutil.copyfile(programs/name, snapshot/name)
            inputs[name] = hashlib.sha256((snapshot/name).read_bytes()).hexdigest()
        programs = snapshot
    # Publish frozen inputs before the VM run so failure records retain them too.
    (proof/(arch+"-inputs.json")).write_text(json.dumps(inputs,indent=2)+"\n")
    expected = {"count.txt": b"21254\n", "copy.bin": shell.DATA, "a.txt": shell.DATA}
    std_tools = volume is None and (
        (programs / "base64").is_file() if programs else bool(os.environ.get("DOSRS_UUTILS_ARCHIVE"))
    )
    fixture_sha256 = {}
    if volume is None:
        data.mkdir()
        reference = shell.script_fixture(data, work)
        expected.update(reference)
        if shell_sweep:
            expected.update({"cold.out": reference["here.out"], "cold.rc": reference["here.rc"]})
        if std_tools:
            expected.update({"encoded.txt": base64.encodebytes(shell.DATA), "decoded.bin": shell.DATA})
        (data/"a.txt").write_bytes(shell.DATA)
        fixture_sha256 = {str(path.relative_to(data)): hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in sorted(data.rglob("*")) if path.is_file()}
    (proof/(arch+"-fixtures.json")).write_text(json.dumps(fixture_sha256,indent=2)+"\n")
    installer.assemble(arch,disk,work,volume,data if volume is None else None,trace=True,kernel=kernel,programs=programs)
    table=partitions(disk)
    app=work/"fdcheck.elf"
    subprocess.run([sys.executable,qemu.ROOT/"scripts/native-cc.py","--arch",arch,"-o",app,
                    qemu.ROOT/"native-exec/examples/fdcheck.c"],check=True)
    inputs["fdcheck"] = hashlib.sha256(app.read_bytes()).hexdigest()
    fat=table[0x0c]
    subprocess.run(["mcopy","-i",f"{disk}@@{fat[0]*512}",app,"::FDCHECK.ELF"],check=True)
    if volume is None:
        for name in shell.PROBES:
            probe = work / name
            if probes is None:
                shell.build_probe(arch, probe)
            else:
                shutil.copyfile(probes / name, probe)
            inputs[name] = hashlib.sha256(probe.read_bytes()).hexdigest()
            subprocess.run(["mcopy", "-i", f"{disk}@@{fat[0]*512}", probe, "::" + name], check=True)
    (proof/(arch+"-inputs.json")).write_text(json.dumps(inputs,indent=2)+"\n")
    serial=proof/(arch+"-install.serial")
    shell.exercise(boot(arch,disk,work),arch,mode,work,serial,dual_volume=True,
                   grammar=shell_sweep,std_tools=std_tools)
    second=Shell(boot(arch,disk,work),proof/(arch+"-cold.serial"),work)
    try:
        second.wait(b"A:\\> ", timeout=180)
        second.command("sh",prompt=b"$ ")
        second.command("cat count.txt",prompt=b"$ ",expect=b"21254\r\n")
        if shell_sweep:
            second.command("sh here.sh > cold.out", prompt=b"$ ", timeout=300)
            second.command("echo $? > cold.rc", prompt=b"$ ")
        second.command("exit",prompt=b"A:\\> ")
        second.command("hello.com",prompt=b"A:\\> ",expect=b"GUEST HELLO COM")
    finally:
        second.close()
    qemu.check_native_boot(proof / (arch+"-cold.serial"), arch, mode)
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
        mismatches=[]
        for name,reference_bytes in expected.items():
            actual=(restored/name).read_bytes()
            if actual!=reference_bytes:
                (proof/(arch+"-"+name+".actual")).write_bytes(actual)
                (proof/(arch+"-"+name+".expected")).write_bytes(reference_bytes)
                mismatches.append(name)
            result[name]={"bytes":len(actual),"sha256":hashlib.sha256(actual).hexdigest(),
                          "matches":actual==reference_bytes}
            (proof/(arch+"-"+name)).write_bytes(actual)
        if (restored/"gone.tmp").exists():raise ValueError("unlinked file survived")
        (proof/(arch+".json")).write_text(json.dumps({"arch":arch,"kernel_bits":32 if arch=="i686" else 64,"firmware":mode,"shell_sweep":shell_sweep,"files":result,
                "qemu":subprocess.check_output(["qemu-system-"+qemu.ARCH[arch][0],"--version"], text=True).splitlines()[0],
                "partitions":{hex(k):v for k,v in table.items()},"inputs":inputs,
                "fixture_sha256":fixture_sha256,"passed":not mismatches,
                "mismatches":mismatches},indent=2)+"\n")
        if mismatches:
            raise ValueError("Linux recovered different files: " + ", ".join(mismatches))
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
    cli.add_argument("--kernel",type=Path,help="use a frozen kernel for the selected native architecture")
    cli.add_argument("--programs",type=Path,help="use that architecture's sh, cat, wc, echo and optional base64 for the root volume")
    cli.add_argument("--probes",type=Path,help="use that architecture's execprog and dirprobe ELF probes")
    args=cli.parse_args()
    if args.root_image and (args.programs or args.probes):
        cli.error("--programs and --probes require building a fresh root volume")
    args.proof.mkdir(parents=True,exist_ok=True)
    if args.archive and args.archive.exists():
        raise FileExistsError(args.archive)
    with tempfile.TemporaryDirectory(dir=args.proof, prefix="."+args.arch+"-") as directory:
        staged = Path(directory)
        try:
            with tempfile.TemporaryDirectory(dir=args.ram_dir) as work:
                run(args.arch,Path(work),staged,args.root_image,args.archive,args.kernel,args.programs,args.probes)
        except BaseException:
            failures = args.proof / "failed"
            failures.mkdir(exist_ok=True)
            shutil.copytree(staged, failures / staged.name.removeprefix("."))
            raise
        # Publish the JSON manifest after the logs and reference bytes it describes.
        for file in sorted(staged.iterdir(), key=lambda file: file.suffix == ".json"):
            os.replace(file, args.proof / file.name)
