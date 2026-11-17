#!/usr/bin/env python3
"""Exercise the live native shell, DOS coexistence and the shared FAT volume."""
import argparse
import hashlib
import re
from pathlib import Path
import subprocess
import tempfile

import qemu

from qemu_shell import Shell

PAYLOAD = bytes(range(256)) * 78 + b"native DOS-RS\x00\xff"


def check(work, proof, arch, firmware, cpus, fat):
    kernel = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    disk = work / "c.img"
    with disk.open("wb") as f:
        f.truncate(({12: 2, 16: 16, 32: 48}[fat] if fat else 4) << 20)
    if fat:
        subprocess.run(["mkfs.fat", "-F", str(fat), "-s", "1", disk], check=True)
    else:
        subprocess.run(["mformat", "-i", disk, "::"], check=True)

    def install(name, contents):
        subprocess.run(["mcopy", "-o", "-i", disk, "-", "::" + name], input=contents, check=True)

    subprocess.run(["mmd", "-i", disk, "::other"], check=True)
    install("other/seed.txt", b"other terminal")
    install("a.txt", PAYLOAD)
    install("seed.txt", b"same name, new file")
    install("count.txt", b"previous result must be truncated\n")
    log = proof / (f"{arch}-fat{fat}.log" if fat else f"{arch}.log")
    cmd = qemu.boot_args(kernel, arch, work / "boot", cpus, firmware)
    cmd += ["-drive", f"if=ide,index=0,format=raw,file={disk}"]
    shell = Shell(cmd, log, work)
    wait, command, enter, key = shell.wait, shell.command, shell.enter, shell.key
    count = str(len(PAYLOAD)).encode()
    try:
        wait(b"DOS / " + arch.encode())
        wait(b"A:\\> ")
        command("c:")
        command("cat a.txt | wc -c > count.txt")
        command("type count.txt", expect=b"\r\n" + count + b"\n")
        command("cat < a.txt | cat > copy.bin")
        command("cat seed.txt >> append.txt")
        command("cat seed.txt >> append.txt")
        command("fdcheck gone.tmp", expect=b"unlink: ENOENT; old fd: original!; new file: replacement")
        command("cat gone.tmp", expect=b"cat: gone.tmp: No such file or directory")
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
        command("cat gone.tmp 2>error.txt | wc -c > zero.txt")
        command("echo $?", expect=b"\r\n0\r\n")
        command("unlink keys.txt")
        command("cat keys.txt", expect=b"cat: keys.txt: No such file or directory")
        command("cat a.txt | wc -c > count.txt")
        enter("cat - seed.txt > resumed.txt")
        key("alt-f2")
        command("cd other", prompt=b"C:\\OTHER> ")
        key("alt-f1")
        start = log.stat().st_size
        key("ctrl-d")
        wait(b"C:\\> ", start)
    finally:
        shell.close()

    trace = log.read_bytes()
    if b"EXEC foreground path=/dos/a/bin/sh\r\n" not in trace:
        raise RuntimeError(f"{arch}: bootstrap shell did not run from the FAT boot volume")
    executions = re.findall(rb"EXEC pid=\d+ path=([^\r\n]+)\r\n", trace)
    for name in ("cat", "wc", "unlink"):
        path = f"/dos/a/bin/{name}".encode()
        if path not in executions:
            raise RuntimeError(f"{arch}: no user exec of {path!r} on the FAT boot volume")

    def read(name):
        return subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])
    if read("count.txt") != count + b"\n" or read("hold.txt") != b"same name, new file":
        raise ValueError(f"{arch}: incorrect redirected bytes")
    if read("append.txt") != b"same name, new file" * 2:
        raise ValueError(f"{arch}: append lost existing bytes")
    if read("resumed.txt") != b"same name, new file":
        raise ValueError(f"{arch}: resumed process lost its current directory")
    if read("error.txt") != b"cat: gone.tmp: No such file or directory\n":
        raise ValueError(f"{arch}: incorrect cat stderr")
    if read("zero.txt") != b"0\n":
        raise ValueError(f"{arch}: empty stream count is wrong")
    if read("copy.bin") != PAYLOAD or read("a.txt") != PAYLOAD:
        raise ValueError(f"{arch}: binary stream mismatch")
    if fat:
        subprocess.run(["fsck.fat", "-n", disk], check=True)
    print(f"{arch}/{firmware}, FAT{fat or ' (automatic)'}, {cpus} CPU: count={len(PAYLOAD)}, "
          f"copy SHA-256={hashlib.sha256(PAYLOAD).hexdigest()}, unlink/dup/EOF, seek, append, terminal cwd and DOS COM passed", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", type=Path, required=True, help="mounted RAM disk for writable VM images")
    parser.add_argument("--proof", type=Path, default=qemu.ROOT / "target/posix-proof")
    parser.add_argument("--fat", choices=[12, 16, 32], action="append", type=int,
                        help="select a FAT format using dosfstools; repeat to exercise several")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch, firmware, cpus in [("i686", "bios", 1), ("x86_64", "uefi", 4)]:
        for fat in args.fat or [None]:
            with tempfile.TemporaryDirectory(dir=args.ram_dir.resolve()) as work:
                check(Path(work), args.proof, arch, firmware, cpus, fat)


if __name__ == "__main__":
    main()
