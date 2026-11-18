#!/usr/bin/env python3
"""Check native musl timers, then recover the report from a stopped RAM disk."""
import argparse
from contextlib import ExitStack
import fcntl
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

PASSED = b"timers: values, overruns, shared signal, absolute clock, fork, exec, exit and handler passed\n"


def check(arch, args):
    mode = "bios" if arch == "i686" else "uefi"
    proof = args.proof / arch
    proof.mkdir(parents=True, exist_ok=True)
    source_kernel = args.kernel or qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    with tempfile.TemporaryDirectory(prefix="posix-timers-", dir=args.ram_dir) as tmp:
        work = Path(tmp)
        kernel = work / "kernel.efi"
        shutil.copyfile(source_kernel, kernel)
        binary = work / "timers.elf"
        link = [] if args.linker_dir is None else ["-B" + str(args.linker_dir) + "/", "-fuse-ld=lld"]
        regressions = {
            "polltest": qemu.ROOT / "tests/native-poll/poll.c",
            "sigtest": qemu.ROOT / "ports/musl/tests/sigwait.c",
            "socktest": qemu.ROOT / "ports/musl/tests/socket_spawn.c",
            "limtest": qemu.ROOT / "ports/musl/tests/limits.c",
        }
        libc_digest = None
        with ExitStack() as resources:
            if not args.elf_dir:
                sysroot = qemu.ROOT / "target/musl" / arch / "sysroot"
                lock = resources.enter_context((sysroot.parent / "build.lock").open("a"))
                fcntl.flock(lock, fcntl.LOCK_SH)
                libc_digest = hashlib.sha256((sysroot / "lib/libc.a").read_bytes()).hexdigest()
            for name, source in {"timers": qemu.ROOT / "ports/musl/tests/timers.c", **regressions}.items():
                output = work / (name + ".elf")
                if args.elf_dir:
                    shutil.copyfile(args.elf_dir / arch / output.name, output)
                else:
                    subprocess.run([sys.executable, qemu.ROOT / "ports/musl/cc.py", "--arch", arch,
                                    "-o", output, "--", *link, source], check=True)
                (proof / "elf").mkdir(exist_ok=True)
                shutil.copyfile(output, proof / "elf" / output.name)
        disk = work / "c.img"
        with disk.open("wb") as file:
            file.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        subprocess.run(["mcopy", "-i", disk, *sorted(work.glob("*.elf")), "::"], check=True)
        command = qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
        command.extend(["-drive", f"if=ide,format=raw,file={disk}"])
        guest = Shell(command, proof / "serial.log", work)
        failure = None
        try:
            guest.wait(b"A:\\> ", timeout=240)
            guest.command("c:", timeout=120)
            guest.command("timers.elf > timers.out", timeout=240)
            guest.command("echo $?", expect=b"\r\n0\r\n")
            for command, marker in [
                ("polltest.elf", b"poll: descriptors, blocking pipe, peer close, timeout and signal mask passed"),
                ("sigtest.elf", b"sigwait: process, targeted thread, competing waiters, realtime order, sender, child and timeout passed"),
                ("socktest.elf issue8", b"local sockets and posix_spawn passed"),
                ("limtest.elf", b"limits: stack growth, syscall entry, fork, exec and failed exec passed"),
            ]:
                guest.command(command, timeout=240, expect=marker)
                guest.command("echo $?", expect=b"\r\n0\r\n")
        except RuntimeError as error:
            failure = error
        finally:
            guest.close()
        qemu.check_native_boot(guest.log, arch, mode)
        report = subprocess.check_output(["mcopy", "-i", disk, "::timers.out", "-"])
        (proof / "timers.out").write_bytes(report)
        if failure:
            raise failure
        spawned = subprocess.check_output(["mcopy", "-i", disk, "::spawn.out", "-"])
        if spawned != b"parent":
            raise RuntimeError(f"stopped posix_spawn file differs: {spawned!r}")
        (proof / "spawn.out").write_bytes(spawned)
        match = re.fullmatch(rb"periodic value=([0-9a-f]+) overrun=([0-9]+)\n" + re.escape(PASSED), report)
        value = 0xabcdef90 if arch == "i686" else 0x12345678abcdef90
        if not match or int(match[1], 16) != value or int(match[2]) < 5:
            raise RuntimeError(f"stopped timer report differs: {report!r}")
        checker = shutil.which("fsck.fat") or "/opt/homebrew/opt/dosfstools/sbin/fsck.fat"
        with (proof / "fsck.log").open("wb") as log:
            subprocess.run([checker, "-n", disk], stdout=log, stderr=subprocess.STDOUT, check=True)
        hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest()
                  for name, path in [("kernel", kernel), ("ELF", binary), ("disk", disk)]}
        if libc_digest is not None:
            hashes["libc"] = libc_digest
        result = {"arch": arch, "kernel_bits": 32 if arch == "i686" else 64,
                  "firmware": mode, "sha256": hashes, "sigval": value,
                  "linker_options": link,
                  "build_mode": "prebuilt" if args.elf_dir else "compile",
                  "overrun": int(match[2]),
                  "unsupported": ["SIGEV_THREAD", "SIGEV_THREAD_ID", "SA_SIGINFO handler/ucontext"],
                  "regression_elf_sha256": {name: hashlib.sha256((work / (name + ".elf")).read_bytes()).hexdigest()
                                             for name in regressions},
                  "qemu": subprocess.check_output(
                      ["qemu-system-" + qemu.ARCH[arch][0], "--version"], text=True).splitlines()[0]}
        (proof / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"{arch}/{mode}: native POSIX timer assertions and stopped report passed", flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=list(qemu.ARCH), action="append")
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/posix-timers-proof")
    cli.add_argument("--kernel", type=Path, help="frozen kernel for exactly one --arch")
    cli.add_argument("--linker-dir", type=Path)
    cli.add_argument("--elf-dir", type=Path, help="replay prebuilt ARCH/{timers,polltest,sigtest,socktest,limtest}.elf")
    args = cli.parse_args()
    if args.kernel and len(args.arch or ()) != 1:
        cli.error("--kernel requires one --arch")
    args.ram_dir = args.ram_dir.resolve(strict=True)
    for arch in args.arch or qemu.ARCH:
        check(arch, args)
