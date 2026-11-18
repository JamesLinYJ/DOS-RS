#!/usr/bin/env python3
"""Check native shell directories and symlinks on an installed RAM Btrfs root."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile

import qemu
from qemu_shell import Shell


def module(name):
    spec = importlib.util.spec_from_file_location(name, qemu.ROOT / "scripts" / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def fixture(base, directories, tree, work, probe, expect_failure):
    expected = directories.directory_fixture(base, tree, work)
    (tree / "up").mkdir()
    links = {"alias": "OTHER/INNER", "up/portal": "../OTHER/INNER",
             "dangling": "missing-directory", "cycle": "cycle"}
    for path, target in links.items():
        (tree / path).symlink_to(target)
    (work / "directory-links.json").write_text(json.dumps(
        {path: str((tree / path).readlink()) for path in links}, indent=2) + "\n")
    shutil.copyfile(qemu.ROOT / "tests/user-shell/directory-links.sh", tree / "dirlinks.sh")
    expected["links.out"] = (qemu.ROOT / "tests/user-shell/directory-links.expected").read_bytes()
    expected["links.rc"] = b"83\n"
    if probe:
        shutil.copyfile(probe, tree / "cwdprobe")
        (tree / "cwdprobe").chmod(0o755)
        expected["cwdedge.out"] = b"" if expect_failure else (
            b"cwd: long physical path, ERANGE and relative Shell cd passed\n"
            b"cwd: unlinked directory and Shell cd -Pe passed\n"
            b"cwd: physical pwd preserves raw pathname bytes\n"
            b"cwd: search-only directory, denied cd and output rollback passed\n")
        expected["cwdedge.err"] = (
            b"cwdprobe: directory assertions: File name too long\n" if expect_failure else b"")
        expected["cwdedge.rc"] = b"1\n" if expect_failure else b"0\n"
        (work / "cwd-probe.json").write_text(json.dumps(dict(
            sha256=hashlib.sha256((tree / "cwdprobe").read_bytes()).hexdigest(),
            expected_failure=expect_failure,
            expectation="old kernel returns ENAMETOOLONG" if expect_failure else "long and unnamed cwd pass",
        ), indent=2) + "\n")
    return expected


def exercise(base, args, arch, mode, root, serial, probe=False, **options):
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
        vm.command("sh dirlinks.sh > links.out", prompt=b"$ ", timeout=300)
        vm.command("echo $? > links.rc", prompt=b"$ ")
        if probe:
            vm.command("/cwdprobe > cwdedge.out 2> cwdedge.err", prompt=b"$ ", timeout=600)
            vm.command("echo $? > cwdedge.rc", prompt=b"$ ")
        vm.command("exit 7")
        vm.command("echo $?", expect=b"7\r\n")
        vm.command("a:", prompt=b"A:\\> ")
        vm.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
    finally:
        vm.close()
    qemu.check_native_boot(cold, arch, mode)
    if probe and b"path=/cwdprobe\r\n" not in cold.read_bytes():
        raise RuntimeError("missing independently executed Rust cwd probe")


def run(arch, work, proof, archive, kernel, programs, probes, cwd_probe=None, expect_failure=False):
    installer = module("qemu-posix-install")
    directories = module("qemu-shell-directories")
    original_module = installer.module

    def installed_module(name):
        result = original_module(name)
        if name == "qemu-user-shell":
            original_fixture, original_exercise = result.script_fixture, result.exercise
            result.script_fixture = lambda *args: fixture(
                original_fixture, directories, *args, cwd_probe, expect_failure)
            result.exercise = lambda *args, **options: exercise(
                original_exercise, *args, probe=bool(cwd_probe), **options)
        return result

    installer.module = installed_module
    try:
        installer.run(arch, work, proof, None, archive, kernel, programs, probes)
    finally:
        links = work / "directory-links.json"
        if links.exists():
            shutil.copyfile(links, proof / (arch + "-directory-links.json"))
        probe = work / "cwd-probe.json"
        if probe.exists():
            shutil.copyfile(probe, proof / (arch + "-cwd-probe.json"))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", type=Path, required=True)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--arch", choices=qemu.ARCH, required=True)
    parser.add_argument("--kernel", type=Path)
    parser.add_argument("--programs", type=Path)
    parser.add_argument("--probes", type=Path)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--cwd-probe", type=Path, help="additional architecture-specific Rust cwd ELF")
    parser.add_argument("--expect-long-path-failure", action="store_true",
                        help="negative control: require the old kernel's ENAMETOOLONG result")
    options = parser.parse_args()
    if options.expect_long_path_failure and options.cwd_probe is None:
        parser.error("the negative control requires --cwd-probe")
    options.proof.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix=f"directory-btrfs-{options.arch}-", dir=options.ram_dir))
    try:
        run(options.arch, work, options.proof, options.archive,
            options.kernel, options.programs, options.probes,
            options.cwd_probe, options.expect_long_path_failure)
    except BaseException:
        print(f"{options.arch}: failed RAM fixture retained at {work}", flush=True)
        raise
    else:
        shutil.rmtree(work)
