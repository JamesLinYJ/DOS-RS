#!/usr/bin/env python3
"""Exercise the live native shell, DOS coexistence and the shared FAT volume."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import time

import qemu

KEYS = {" ": "spc", ".": "dot", "-": "minus", ":": "shift-semicolon",
        "$": "shift-4", "?": "shift-slash", "|": "shift-backslash", ">": "shift-dot", "<": "shift-comma", "/": "slash"}
PAYLOAD = bytes(range(256)) * 78 + b"native DOS-RS\x00\xff"


def check(work, proof, arch, firmware, cpus):
    kernel = qemu.build(arch, qemu.ROOT / "target")
    disk = work / "c.img"
    with disk.open("wb") as f:
        f.truncate(4 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)

    def install(name, contents):
        subprocess.run(["mcopy", "-o", "-i", disk, "-", "::" + name], input=contents, check=True)

    install("a.txt", PAYLOAD)
    install("seed.txt", b"same name, new file")
    install("count.txt", b"previous result must be truncated\n")
    log = proof / f"{arch}.log"
    log.write_bytes(b"")
    cmd = qemu.boot_args(kernel, arch, work / "boot", cpus, firmware)
    cmd += ["-drive", f"if=ide,index=0,format=raw,file={disk}", "-display", "none",
            "-serial", "file:" + str(log), "-monitor", "stdio", "-nic", "none"]
    env = dict(os.environ, TMPDIR=str(work))
    vm = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, env=env)

    def monitor(command):
        vm.stdin.write((command + "\n").encode())
        vm.stdin.flush()

    def key(name):
        monitor("sendkey " + name + " 20")
        time.sleep(0.045)

    def enter(line):
        for ch in line:
            key(KEYS.get(ch, ch))
        key("ret")

    def wait(marker, since=0):
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline and vm.poll() is None:
            data = log.read_bytes()
            if marker in data[since:]:
                return
            time.sleep(0.06)
        raise RuntimeError(f"{arch}: missing {marker!r}\n{log.read_bytes()[-1400:]!r}")

    def command(line, prompt=b"C:\\> ", expect=None):
        start = log.stat().st_size
        enter(line)
        wait(prompt, start)
        if expect is not None:
            wait(expect, start)

    count = str(len(PAYLOAD)).encode()
    try:
        wait(b"DOS / " + arch.encode())
        wait(b"A:\\> ")
        command("c:")
        command("cat a.txt | wc -c > count.txt")
        command("type count.txt", expect=b"\r\n" + count + b"\n")
        command("cat < a.txt | cat > copy.bin")
        command("fdcheck gone.tmp", expect=b"unlink: ENOENT; old fd: original!; new file: replacement")
        command("cat gone.tmp", expect=b"cat: no such file or directory")
        # Leave two processes on VT1 waiting for input, then run a DOS binary on VT2.
        start = log.stat().st_size
        enter("cat | wc -c > keys.txt")
        key("alt-f2")
        wait(b"DOS terminal 2", start)
        wait(b"A:\\> ", start)
        command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
        key("alt-f1")
        start = log.stat().st_size
        enter("hello")
        key("ctrl-d")
        wait(b"C:\\> ", start)
        command("type keys.txt", expect=b"\r\n6\n")
        # A native writer sleeps on stdin while DOS renames and deletes its file.
        enter("cat > hold.txt")
        key("alt-f2")
        command("c:")
        command("ren hold.txt moved.txt", expect=b"File renamed.")
        command("copy seed.txt hold.txt", expect=b"File copied.")
        command("del moved.txt", expect=b"File deleted.")
        key("alt-f1")
        start = log.stat().st_size
        enter("detached writer")
        key("ctrl-d")
        wait(b"C:\\> ", start)
        command("echo $?", expect=b"\r\n0\r\n")
        command("cat hold.txt", expect=b"same name, new file")
        command("fdcheck seed.txt", expect=b"fdcheck: file exists")
        command("echo $?", expect=b"\r\n1\r\n")
        command("cat gone.tmp | wc -c > zero.txt", expect=b"cat: no such file or directory")
        command("echo $?", expect=b"\r\n0\r\n")
        command("unlink keys.txt")
        command("cat keys.txt", expect=b"cat: no such file or directory")
        command("cat a.txt | wc -c > count.txt")
    finally:
        if vm.poll() is None:
            monitor("quit")
            vm.wait(timeout=8)
        vm.stdin.close()

    def read(name):
        return subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])
    if read("count.txt") != count + b"\n" or read("hold.txt") != b"same name, new file":
        raise ValueError(f"{arch}: incorrect redirected bytes")
    if read("zero.txt") != b"0\n":
        raise ValueError(f"{arch}: empty stream count is wrong")
    if read("copy.bin") != PAYLOAD or read("a.txt") != PAYLOAD:
        raise ValueError(f"{arch}: binary stream mismatch")
    print(f"{arch}/{firmware}, {cpus} CPU: count={len(PAYLOAD)}, "
          f"copy SHA-256={hashlib.sha256(PAYLOAD).hexdigest()}, unlink/dup/EOF and DOS COM passed", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", type=Path, required=True, help="mounted RAM disk for writable VM images")
    parser.add_argument("--proof", type=Path, default=qemu.ROOT / "target/posix-proof")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch, firmware, cpus in [("i686", "bios", 1), ("x86_64", "uefi", 4)]:
        with tempfile.TemporaryDirectory(dir=args.ram_dir.resolve()) as work:
            check(Path(work), args.proof, arch, firmware, cpus)


if __name__ == "__main__":
    main()
