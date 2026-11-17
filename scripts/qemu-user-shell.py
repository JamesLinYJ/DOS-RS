#!/usr/bin/env python3
"""Boot the native shell and verify its executable paths and redirected files on a stopped RAM disk."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

DATA = bytes(range(256)) * 83 + b"DOS-RS"


def exercise(args, arch, mode, root, serial, dual_volume=False, job_control=False, grammar=False, std_tools=False):
    vm = Shell(args, serial, root)
    try:
        vm.wait(b"A:\\> ")
        vm.command("c:")
        if dual_volume:
            vm.command("fdcheck.elf", expect=b"unlink, shared offset, pipe EOF OK")
        vm.command("sh", prompt=b"$ ")
        vm.command("cat a.txt | wc -c > count.txt", prompt=b"$ ")
        vm.command("cat count.txt", prompt=b"$ ", expect=str(len(DATA)).encode() + b"\r\n")
        vm.command("cat a.txt | cat > copy.bin", prompt=b"$ ", timeout=180 if dual_volume else 40)
        vm.command("absent", prompt=b"$ ", expect=b"command not found")
        vm.command("echo $?", prompt=b"$ ", expect=b"127\r\n")
        if std_tools:
            vm.command("base64 a.txt > encoded.txt", prompt=b"$ ", timeout=240)
            vm.command("base64 -d encoded.txt > decoded.bin", prompt=b"$ ", timeout=240)
        if grammar:
            vm.command('sh control.sh one "two words" > subst.out', prompt=b"$ ", timeout=300 if dual_volume else 120)
            vm.command("echo $? > subst.rc", prompt=b"$ ")
        if job_control:
            offset = serial.stat().st_size
            vm.enter("cat")
            vm.wait(b"path=/bin/cat", offset)
            vm.key("ctrl-z")
            vm.wait(b"Stopped cat", offset)
            vm.command("jobs", prompt=b"$ ", expect=b"Stopped cat")
            offset = serial.stat().st_size
            vm.enter("fg")
            vm.key("ctrl-c")
            vm.wait(b"$ ", offset)
            vm.command("echo $?", prompt=b"$ ", expect=b"130\r\n")
        vm.command("exit 7")
        vm.command("echo $?", expect=b"7\r\n")
    finally:
        vm.close()
    trace = serial.read_bytes()
    for path in (b"/bin/sh", b"/bin/cat", b"/bin/wc", b"/bin/echo"):
        if b"path=" + path + b"\r\n" not in trace:
            raise RuntimeError(f"{arch}: missing executed path {path!r}")
    print(f"{arch}/{mode}: guest ran /bin/sh, /bin/cat, /bin/wc and /bin/echo", flush=True)


def check(arch, root, proof):
    mode = "bios" if arch == "i686" else "uefi"
    kernel = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    programs = root / "bin"
    subprocess.run([sys.executable, qemu.ROOT / "scripts/build-rust-tools.py", "--arch", arch,
                    "--output", programs], check=True)
    disk = root / "c.img"
    with disk.open("wb") as f:
        f.truncate(8 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    subprocess.run(["mmd", "-i", disk, "::bin"], check=True)
    for name in ("sh","cat","wc","echo"):
        subprocess.run(["mcopy", "-i", disk, programs/name, "::bin/"+name], check=True)
    subprocess.run(["mcopy", "-i", disk, "-", "::a.txt"], input=DATA, check=True)
    source = qemu.ROOT / "tests/user-shell/control.sh"
    subprocess.run(["mcopy", "-i", disk, source, "::control.sh"], check=True)
    reference = subprocess.run(["/bin/sh", source, "one", "two words"],
                               cwd=root, env=dict(os.environ, LC_ALL="C"), capture_output=True)
    if reference.returncode != 13 or reference.stderr:
        raise RuntimeError(f"host shell reference failed: {reference!r}")
    serial = proof / (arch+".serial")
    args=qemu.boot_args(kernel, arch, root / "boot", 4, mode, esp_mib=36)
    exercise(args + ["-drive", f"if=ide,format=raw,file={disk}"], arch, mode, root, serial, job_control=True, grammar=True)
    subprocess.run(["fsck.fat", "-n", disk], check=True)
    for name, expected in [("count.txt", f"{len(DATA)}\n".encode()), ("copy.bin", DATA),
                           ("subst.out", reference.stdout), ("subst.rc", b"13\n")]:
        result = subprocess.check_output(["mcopy","-i",disk,"::"+name,"-"])
        (proof/(arch+"-"+name)).write_bytes(result)
        if result != expected:
            (proof/(arch+"-"+name+".expected")).write_bytes(expected)
            raise RuntimeError(f"{arch}: stopped {name} differs")
        print(f"{arch}/{mode} {name}: {len(result)} bytes sha256 {hashlib.sha256(result).hexdigest()}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=qemu.ROOT/"target/user-shell-proof")
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    options=parser.parse_args()
    options.proof.mkdir(parents=True, exist_ok=True)
    for arch in options.arch or qemu.ARCH:
        with tempfile.TemporaryDirectory(dir=options.ram_dir) as tmp:
            check(arch,Path(tmp),options.proof)
