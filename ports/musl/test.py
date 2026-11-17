#!/usr/bin/env python3
"""Build upstream musl and run its static ELF on both DOS-RS schedulers."""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile
from build import build, ROOT, PORT
from cc import compile

sys.path.insert(0, str(ROOT / "scripts"))
import qemu
from qemu_shell import Shell

PASSED = b"musl static TLS pthreads condvar malloc stdio fork exec wait sleep signal OK"


def check(arch, archive, ram, proof):
    sysroot = build(arch, archive, ROOT / "target/musl" / arch)
    kernel = qemu.build(arch, ROOT / "target")
    with tempfile.TemporaryDirectory(dir=ram) as temporary:
        work = Path(temporary)
        binary = work / "libtest.elf"
        socket_test = work / "socktest.elf"
        compile(arch, [PORT / "tests/socket_spawn.c"], socket_test)
        poll_test = work / "polltest.elf"
        sig_test = work / "sigtest.elf"
        compile(arch, [PORT / "tests/sigwait.c"], sig_test)
        tty_test = work / "ttytest.elf"
        compile(arch, [PORT / "tests/terminal.c"], tty_test)
        compile(arch, [ROOT / "tests/native-poll/poll.c"], poll_test)
        compile(arch, [PORT / "tests/libc.c"], binary)
        headers = subprocess.check_output([arch + "-elf-readelf", "-lW", binary])
        if b" TLS " not in headers or b"INTERP" in headers:
            raise RuntimeError("missing static ELF TLS program header")
        (proof / (arch + "-elf.txt")).write_bytes(headers)
        disk = work / "c.img"
        with disk.open("wb") as f:
            f.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        subprocess.run(["mcopy", "-i", disk, binary, poll_test, tty_test, sig_test, socket_test, "::"], check=True)
        mode = "bios" if arch == "i686" else "uefi"
        launch = qemu.boot_args(kernel, arch, work / "boot", 4, mode, esp_mib=36)
        launch += ["-accel", "tcg,thread=multi", "-drive", "if=ide,format=raw,file=" + str(disk)]
        shell = Shell(launch, proof / (arch + ".log"), work)
        try:
            shell.wait(b"A:\\> ")
            shell.command("c:")
            shell.command("libtest.elf probe")
            shell.command("echo $?")
            report = shell.log.read_bytes()
            if PASSED not in report or b"echo $?\r\n0\r\n" not in report:
                raise RuntimeError("libc assertions failed: " + repr(report[-500:]))
            at = shell.log.stat().st_size
            shell.enter("ttytest.elf")
            shell.wait(b"TTY RAW READY\n", at)
            shell.key("ctrl-c")
            shell.key("ctrl-d")
            shell.wait(b"TTY INTERBYTE READY\n", at)
            shell.key("x")
            shell.key("y")
            shell.wait(b"TTY CANONICAL READY\r\n", at)
            for key in ("a", "b", "backspace", "c", "ret"):
                shell.key(key)
            shell.wait(b"TTY SIGNAL READY\r\n", at)
            shell.key("ctrl-w")
            shell.wait(b"TTY EOF READY\r\n", at)
            for key in ("d", "e", "ctrl-d", "ctrl-d"):
                shell.key(key)
            shell.wait(b"tty: descriptors, VGA 80x25, canonical, raw, timers, echo, signals and EOF passed", at)
            shell.wait(b"C:\\> ", at)
            trace = shell.log.read_bytes()[at:]
            if b"ab\x08 \x08c\r\n" not in trace or b"^C" in trace or b"^D" in trace:
                raise RuntimeError("incorrect keyboard echo: " + repr(trace))
            shell.command("echo $?", expect=b"\r\n0\r\n")
            shell.command("sigtest.elf", expect=b"sigwait: process, targeted thread, competing waiters, realtime order, sender, child and timeout passed")
            shell.command("echo $?", expect=b"\r\n0\r\n")
            shell.command("socktest.elf issue8", expect=b"local sockets and posix_spawn passed")
            shell.command("echo $?", expect=b"\r\n0\r\n")
            shell.command("polltest.elf > poll.out")
            shell.command("echo $?", expect=b"\r\n0\r\n")
        finally:
            shell.close()
        payload = subprocess.check_output(["mcopy", "-i", disk, "::spawn.out", "-"])
        if payload != b"parent":
            raise RuntimeError("spawn file action changed payload: " + repr(payload))
        expected = b"argc=2 sum=21 tls=37\n"
        data = subprocess.check_output(["mcopy", "-i", disk, "::musl.txt", "-"])
        if data != expected:
            raise RuntimeError("stopped disk differs: " + repr(data))
        poll = subprocess.check_output(["mcopy", "-i", disk, "::poll.out", "-"])
        if poll != b"poll: descriptors, blocking pipe, peer close, timeout and signal mask passed\n":
            raise RuntimeError("unexpected poll/ppoll result: " + repr(poll))
        print(f"{arch}/{mode} musl-1.2.6: {sysroot}; {len(data)} stdio bytes matched")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=ROOT / "target/musl-proof")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or list(qemu.ARCH):
        check(arch, args.archive.resolve(), args.ram_dir.resolve(strict=True), args.proof)
