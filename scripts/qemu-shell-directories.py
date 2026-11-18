#!/usr/bin/env python3
"""Repeat the native shell gate, then cold-boot its directory tests on a RAM disk."""
import argparse
import importlib.util
from pathlib import Path
import shutil
import tempfile

import qemu
from qemu_shell import Shell


def directory_fixture(base, fixture, work):
    expected = base(fixture, work)
    for name in ("OTHER/INNER", "LOOK/INNER", "-dir"):
        (fixture / name).mkdir(parents=True)
    shutil.copyfile(qemu.ROOT / "tests/user-shell/directory.sh", fixture / "dirs.sh")
    expected["dirs.out"] = (qemu.ROOT / "tests/user-shell/directory.expected").read_bytes()
    expected["dirs.rc"] = b"79\n"
    return expected


def directory_exercise(base, args, arch, mode, root, serial, **options):
    base(args, arch, mode, root, serial, **options)
    cold = serial.with_name(arch + "-directory.serial")
    vm = Shell(args, cold, root)
    try:
        vm.wait(b"A:\\> ", timeout=180)
        vm.command("c:")
        vm.command("sh", prompt=b"$ ")
        vm.command("cat count.txt", prompt=b"$ ", expect=b"21254\r\n")
        vm.command("sh dirs.sh > dirs.out", prompt=b"$ ", timeout=300)
        vm.command("echo $? > dirs.rc", prompt=b"$ ")
        vm.command("exit 7")
        vm.command("echo $?", expect=b"7\r\n")
        vm.command("a:", prompt=b"A:\\> ")
        vm.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
    finally:
        vm.close()
    qemu.check_native_boot(cold, arch, mode)


def check(arch, ram, proof, kernel, programs, probes):
    spec = importlib.util.spec_from_file_location("shell_gate", qemu.ROOT / "scripts/qemu-user-shell.py")
    shell = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(shell)
    fixture, exercise = shell.script_fixture, shell.exercise
    shell.script_fixture = lambda *args: directory_fixture(fixture, *args)
    shell.exercise = lambda *args, **options: directory_exercise(exercise, *args, **options)
    work = Path(tempfile.mkdtemp(prefix=f"directory-{arch}-", dir=ram))
    try:
        shell.check(arch, work, proof, kernel, programs, probes)
    except BaseException:
        # A failed disk remains in RAM for diagnosis. The caller decides when
        # its stopped bytes have been recorded and the directory can be removed.
        print(f"{arch}: failed RAM fixture retained at {work}", flush=True)
        raise
    else:
        shutil.rmtree(work)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", type=Path, required=True)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    parser.add_argument("--kernel", type=Path)
    parser.add_argument("--programs", type=Path)
    parser.add_argument("--probes", type=Path)
    options = parser.parse_args()
    if (options.kernel or options.programs or options.probes) and len(options.arch or ()) != 1:
        parser.error("prebuilt inputs require exactly one --arch")
    options.proof.mkdir(parents=True, exist_ok=True)
    for arch in options.arch or qemu.ARCH:
        check(arch, options.ram_dir, options.proof, options.kernel, options.programs, options.probes)
