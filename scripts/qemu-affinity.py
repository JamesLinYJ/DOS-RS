#!/usr/bin/env python3
"""Run a live sched_getaffinity/pthread probe and a standalone Rust nproc ELF."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import qemu
from qemu_shell import Shell


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(arch, ram, proof, gnu_bin, archive):
    mode = "bios" if arch == "i686" else "uefi"
    target = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    gnu = gnu_bin / "nproc"
    version = subprocess.check_output([gnu, "--version"]).splitlines()[0]
    host_all = int(subprocess.check_output([gnu, "--all"]))
    if host_all < 1 or version != b"nproc (GNU coreutils) 9.12":
        raise RuntimeError(f"unexpected GNU oracle {version!r}")
    with tempfile.TemporaryDirectory(dir=ram) as temporary:
        work = Path(temporary)
        kernel = work / "kernel.efi"
        kernel.write_bytes(target.read_bytes())
        subprocess.run(
            [
                sys.executable,
                qemu.ROOT / "scripts/build-uutils.py",
                "--arch",
                arch,
                "--output",
                work,
                "--archive",
                archive,
                "--offline",
                "nproc",
            ],
            check=True,
        )
        probe = work / "aff.elf"
        subprocess.run(
            [
                sys.executable,
                qemu.ROOT / "ports/musl/cc.py",
                "--arch",
                arch,
                "-o",
                probe,
                qemu.ROOT / "tests/native-affinity/online.c",
            ],
            check=True,
        )
        for cpus in (1, 4):
            disk = work / f"c{cpus}.img"
            with disk.open("xb") as f:
                f.truncate(8 << 20)
            subprocess.run(["mformat", "-i", disk, "::"], check=True)
            subprocess.run(["mcopy", "-i", disk, probe, "::aff.elf"], check=True)
            subprocess.run(
                ["mcopy", "-i", disk, work / "nproc", "::nproc.elf"], check=True
            )
            serial = proof / f"{arch}-{cpus}.serial"
            vm = Shell(
                qemu.boot_args(
                    kernel, arch, work / f"boot{cpus}", cpus, mode, esp_mib=36
                )
                + ["-drive", f"if=ide,format=raw,file={disk}"],
                serial,
                work,
            )
            options = [[], ["--all"], ["--ignore", "1"], ["--ignore", "9"]]
            try:
                vm.wait(b"A:\\> ")
                vm.command("c:")
                vm.command(f"aff.elf {cpus} > aff.txt")
                vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
                for i, args in enumerate(options):
                    vm.command(" ".join(["nproc.elf", *args, ">", f"n{i}.txt"]))
                    vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
            finally:
                vm.close()

            def read(name):
                return subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])

            summary = read("aff.txt").decode()
            if (
                summary
                != f"online={cpus} mask={(1 << cpus) - 1:x} foreign-process-and-thread=ok\n"
            ):
                raise RuntimeError(f"affinity probe on {arch}: {summary!r}")
            checks = []
            for i, args in enumerate(options):
                # Compare the same count, normalized to the guest topology. GNU's
                # --all queries the host independently of OMP_NUM_THREADS.
                env = dict(os.environ, LC_ALL="C", OMP_NUM_THREADS=str(cpus))
                env.pop("OMP_THREAD_LIMIT", None)
                ref = subprocess.check_output(
                    [gnu, *([] if args == ["--all"] else args)], env=env
                )
                actual = read(f"n{i}.txt")
                if actual != ref:
                    raise RuntimeError(
                        f"{arch} {cpus} vCPUs {args}: {actual!r} != {ref!r}"
                    )
                checks.append({"argv": args, "stdout": actual.decode().strip()})
            result = dict(
                arch=arch,
                firmware=mode,
                vcpus=cpus,
                probe=summary.strip(),
                nproc=checks,
                gnu_host_all=host_all,
                kernel_sha256=digest(kernel),
                probe_sha256=digest(probe),
                guest_nproc_sha256=digest(work / "nproc"),
                gnu_nproc_sha256=digest(gnu),
            )
            (proof / f"{arch}-{cpus}.json").write_text(
                json.dumps(result, indent=2) + "\n"
            )
            print(
                f"{arch}/{mode} -smp {cpus}: foreign TID, masks, sysconf and nproc passed",
                flush=True,
            )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument(
        "--proof", type=Path, default=qemu.ROOT / "target/native-affinity-proof"
    )
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--gnu-bin", required=True, type=Path)
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or qemu.ARCH:
        verify(arch, args.ram_dir, args.proof, args.gnu_bin, args.archive)
