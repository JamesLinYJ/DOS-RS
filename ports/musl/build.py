#!/usr/bin/env python3
"""Build a static DOS-RS musl sysroot from its verified upstream release."""
import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile

PORT = Path(__file__).resolve().parent
ROOT = PORT.parents[1]
SHA256 = "d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a"


def generated(dest, body):
    if not dest.exists() or dest.read_text() != body:
        dest.write_text(body)


def copy(source, dest):
    if not dest.exists() or dest.read_bytes() != source.read_bytes():
        shutil.copyfile(source, dest)


def prepare(src, target, cpu, archive):
    overlay = PORT / "overlay"
    copy(overlay / "syscall_arch.h", src / "arch" / target / "syscall_arch.h")
    for port, upstream in (
        ("crt1.c", "crt/crt1.c"),
        ("platform.c", "src/thread/dosrs.c"),
        ("fstatat.c", "src/stat/fstatat.c"),
        ("cancel.c", "src/thread/dos_cancel.c"),
        ("setxid.c", "src/unistd/setxid.c"),
        ("pwrite.c", "src/unistd/pwrite.c"),
    ):
        copy(overlay / port, src / upstream)
    for path in (
        "src/process/{}/vfork.s",
        "src/signal/{}/restore.s",
        "src/thread/{}/clone.s",
        "src/thread/{}/__unmapself.s",
        "src/thread/{}/__set_thread_area.s",
        "src/thread/{}/syscall_cp.s",
    ):
        (src / path.format(target)).unlink(missing_ok=True)

    # Keep musl's public cancellation state machine and cleanup handling.
    # Its Linux signal handler normally rewrites ucontext to reenter an assembly
    # stub. DOS-RS interrupts the thread in the kernel and calls a user handler.
    with tarfile.open(archive) as release:
        member = release.extractfile("musl-1.2.6/src/thread/pthread_cancel.c")
        body = member.read().decode()
        header = release.extractfile("musl-1.2.6/include/fcntl.h").read().decode()
    header = header.replace("#define FD_CLOEXEC 1", "#define FD_CLOEXEC 1\n#define FD_CLOFORK 2\n#define O_CLOFORK 0x04000000\n#define F_DUPFD_CLOFORK 1100")
    generated(src / "include/fcntl.h", header)
    begin = body.index("static void _sigaddset(")
    end = body.index("void __testcancel()", begin)
    body = body[:begin] + "hidden void __dos_cancel_signal(int);\n\n" + body[end:]
    body = body.replace(".sa_flags = SA_SIGINFO | SA_RESTART | SA_ONSTACK,",
                        ".sa_flags = 0,")
    body = body.replace(".sa_sigaction = cancel_handler",
                        ".sa_handler = __dos_cancel_signal")
    generated(src / "src/thread/pthread_cancel.c", body)

    table = ROOT / "native-exec/libc/syscalls.def"
    declarations = []
    gates = []
    for line in table.read_text().splitlines():
        name, number, argc = line.split()
        params = ", ".join(["long"] * int(argc)) or "void"
        declarations.append(f"long sys_{name}({params});\n")
        gates.append(f"#define SYS_sys_{name} {number}\nSYSCALL(sys_{name})\n")
    generated(src / "src/internal/dos_calls.h", "".join(declarations))
    template = (cpu / "usys.S").read_text()
    macro = template[template.index("#define SYSCALL"):template.index("SYSCALL(fork)")]
    generated(src / "src/thread" / target / "dos_calls.S",
              '#include "traps.h"\n' + macro + "".join(gates))


def build(arch, archive, work):
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise ValueError("unexpected musl-1.2.6 tarball digest")
    work.mkdir(parents=True, exist_ok=True)
    with (work / "build.lock").open("a") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        return install(arch, archive, work)


def install(arch, archive, work):
    src = work / "musl-1.2.6"
    target = "i386" if arch == "i686" else arch
    cpu = ROOT / "target" / ("user32-cpu" if arch == "i686" else "user64-cpu")
    if not src.exists():
        with tempfile.TemporaryDirectory(dir=work) as tmp:
            subprocess.run(["tar", "-xzf", archive], cwd=tmp, check=True)
            (Path(tmp) / src.name).rename(src)
    prepare(src, target, cpu, archive)
    obj = work / "obj"
    obj.mkdir(exist_ok=True)
    sysroot = work / "sysroot"
    if not (obj / "config.mak").exists():
        flags = "-fno-stack-protector -fno-pic -fno-pie"
        if arch == "x86_64":
            flags += " -mno-red-zone"
        subprocess.run([
            src / "configure", "--target=" + target + "-linux-musl", "--disable-shared",
            "--disable-wrapper", "--prefix=" + str(sysroot), "CC=" + arch + "-elf-gcc",
            "CROSS_COMPILE=" + arch + "-elf-", "CPPFLAGS=-I" + str(cpu), "CFLAGS=" + flags,
        ], cwd=obj, check=True)
    make = shutil.which("gmake") or "make"
    subprocess.run([make, "-j" + str(min(12, os.cpu_count() or 1)),
                    "lib/libc.a", "lib/crt1.o", "install-headers"], cwd=obj, check=True)
    lib = sysroot / "lib"
    lib.mkdir(exist_ok=True)
    for name in ("libc.a", "crt1.o"):
        with tempfile.NamedTemporaryFile(dir=lib, delete=False) as tmp:
            staged = Path(tmp.name)
        try:
            shutil.copyfile(obj / "lib" / name, staged)
            staged.chmod(0o644)
            staged.replace(lib / name)
        finally:
            staged.unlink(missing_ok=True)
    return sysroot


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", required=True, choices=["i686", "x86_64"])
    cli.add_argument("--archive", required=True, type=Path)
    args = cli.parse_args()
    print(build(args.arch, args.archive.resolve(), ROOT / "target/musl" / args.arch))
