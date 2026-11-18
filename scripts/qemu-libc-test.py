#!/usr/bin/env python3
"""Run pinned upstream libc-test ELF cases and retain every guest result."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

spec = importlib.util.spec_from_file_location("coreutils_qa", Path(__file__).with_name("qemu-coreutils.py"))
coreutils = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coreutils)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(arch, args):
    build_dir = args.build_dir / arch
    manifest = json.loads((build_dir / "build.json").read_text())
    if manifest["arch"] != arch:
        raise ValueError("libc-test build architecture mismatch")
    for row in manifest["cases"]:
        row.setdefault("selected", row["status"] in ("built", "build_failed"))
    cases = [row for row in manifest["cases"] if row["status"] == "built"]
    if not cases:
        raise ValueError("no independent libc-test ELF programs were built")
    proof = args.proof / arch
    proof.mkdir(parents=True, exist_ok=True)
    mode = "bios" if arch == "i686" else "uefi"
    source_kernel = args.kernel or qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    with tempfile.TemporaryDirectory(prefix="libc-test-", dir=args.ram_dir) as temporary:
        work = Path(temporary)
        tree = work / "seed"
        for name in ("bin", "dev", "libctest", "tmp"):
            (tree / name).mkdir(parents=True)
        (tree / "tmp").chmod(0o1777)
        kernel = work / "kernel.efi"
        shutil.copyfile(source_kernel, kernel)
        timeout = tree / "bin/timeout"
        programs = {}
        if args.programs:
            for executable in sorted((args.programs / arch).iterdir()):
                if executable.name == timeout.name:
                    raise ValueError("--timeout-dir supplies the timeout ELF")
                binary = tree / "bin" / executable.name
                shutil.copyfile(executable, binary)
                binary.chmod(0o755)
                programs[binary.name] = sha(binary)
        shutil.copyfile(args.timeout_dir / arch / "timeout", timeout)
        timeout.chmod(0o755)
        (tree / "z.txt").write_bytes(b"")
        config = bytearray()
        for row in cases:
            source = Path(row["elf"])
            if sha(source) != row["elf_sha256"]:
                raise ValueError("ELF changed after build: " + row["case"])
            name = row["case"].replace("/", "-") + ".elf"
            shutil.copyfile(source, tree / "libctest" / name)
            (tree / "libctest" / name).chmod(0o755)
            config.extend(bytes([3, 0]) + b"z.txt\0LC_ALL=C\0/bin/timeout\0timeout\0")
            config.extend(str(args.case_timeout).encode() + b"\0/libctest/" + name.encode() + b"\0")
        (tree / "cases.bin").write_bytes(config + b"\0")
        link = [] if args.linker_dir is None else ["-B" + str(args.linker_dir) + "/", "-fuse-ld=lld"]
        subprocess.run([sys.executable, qemu.ROOT / "ports/musl/cc.py", "--arch", arch,
                        "-o", tree / "probe.elf", "--", *link,
                        qemu.ROOT / "tests/native-coreutils/driver.c"], check=True)
        disk = work / "root.img"
        coreutils.format_image(disk, tree, "btrfs", [], proof / "format.log")
        runpy.run_path(str(qemu.ROOT / "scripts/posix-install-image.py"))["seed_devices"](disk)
        command = qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
        command.extend(["-device", "virtio-rng-pci", "-drive", f"if=ide,format=raw,file={disk}"])
        guest = Shell(command, proof / "serial.log", work)
        error = None
        try:
            guest.wait(b"A:\\> ", timeout=240)
            guest.command("c:", timeout=120)
            coreutils.run_cases(guest)
            guest.command("echo $?", expect=b"\r\n0\r\n")
        except RuntimeError as failure:
            error = str(failure)
        finally:
            guest.close()
        qemu.check_native_boot(guest.log, arch, mode)
        read = coreutils.stopped_files(disk, "btrfs", work / "restored", [], proof / "fsck.log")
        try:
            codes, signals = read("status.bin"), read("signals.bin")
        except FileNotFoundError:
            codes, signals = b"", b""
        trace = guest.log.read_bytes()
        for i, row in enumerate(cases):
            result = dict(row)
            if i >= min(len(codes), len(signals)):
                result["status"] = "not_run"
            else:
                prefix = proof / "cases" / row["case"]
                prefix.parent.mkdir(parents=True, exist_ok=True)
                streams = {ext: read(f"q{i:03}.{ext}") for ext in ("out", "err")}
                for ext, data in streams.items():
                    Path(str(prefix) + "." + ext).write_bytes(data)
                status = -signals[i] if signals[i] else codes[i]
                path = b"/libctest/" + (row["case"].replace("/", "-") + ".elf").encode()
                executed = b"path=" + path + b"\r\n" in trace
                result.update(exit_status=status, executed_elf=executed,
                              stdout_sha256=hashlib.sha256(streams["out"]).hexdigest(),
                              stderr_sha256=hashlib.sha256(streams["err"]).hexdigest())
                result["status"] = "passed" if status == 0 and not any(streams.values()) and executed else "failed"
                if status == 124:
                    result["status"] = "timed_out"
            row.update(result)
        manifest.update(firmware=mode, kernel_sha256=sha(kernel), timeout_sha256=sha(timeout),
                        program_sha256=programs,
                        driver_sha256=sha(tree / "probe.elf"), kernel_bits=32 if arch == "i686" else 64,
                        filesystem="btrfs", locale="C", timezone="UTC", runtime_error=error,
                        case_timeout_seconds=args.case_timeout,
                        qemu=subprocess.check_output(
                            ["qemu-system-" + qemu.ARCH[arch][0], "--version"], text=True).splitlines()[0])
        (proof / "result.json").write_text(json.dumps(manifest, indent=2) + "\n")
        passed = sum(row["status"] == "passed" for row in cases)
        print(f"{arch}/{mode}: {passed}/{len(cases)} upstream libc-test cases passed", flush=True)
        return not error and all(row["status"] == "passed"
                                 for row in manifest["cases"] if row["selected"])


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=list(qemu.ARCH), action="append")
    cli.add_argument("--kernel", type=Path)
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--build-dir", default=qemu.ROOT / "target/libc-test", type=Path)
    cli.add_argument("--timeout-dir", required=True, type=Path)
    cli.add_argument("--programs", type=Path, help="optional directories of native ARCH/PROGRAM ELF dependencies")
    cli.add_argument("--proof", default=qemu.ROOT / "target/libc-test-proof", type=Path)
    cli.add_argument("--linker-dir", type=Path)
    cli.add_argument("--case-timeout", default=15, type=int, choices=range(1, 61))
    args = cli.parse_args()
    if args.kernel and len(args.arch or ()) != 1:
        cli.error("--kernel requires exactly one --arch")
    args.ram_dir = args.ram_dir.resolve(strict=True)
    results = [run(arch, args) for arch in args.arch or qemu.ARCH]
    sys.exit(0 if all(results) else 1)
