#!/usr/bin/env python3
"""Check uname dynamic fields against the running guest kernel ABI and GNU output order."""

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path
import qemu
from qemu_shell import Shell


def verify(arch, ram, proof, gnu, archive):
    mode = "bios" if arch == "i686" else "uefi"
    built = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    with tempfile.TemporaryDirectory(dir=ram) as temp:
        work = Path(temp)
        kernel = work / "kernel.efi"
        kernel.write_bytes(built.read_bytes())
        subprocess.run(
            [
                sys.executable,
                qemu.ROOT / "scripts/build-uutils.py",
                "--arch",
                arch,
                "--archive",
                archive,
                "--output",
                work,
                "--offline",
                "arch",
                "uname",
            ],
            check=True,
        )
        probe = work / "uts.elf"
        subprocess.run(
            [
                sys.executable,
                qemu.ROOT / "ports/musl/cc.py",
                "--arch",
                arch,
                "-o",
                probe,
                qemu.ROOT / "tests/native-uts/names.c",
            ],
            check=True,
        )
        disk = work / "c.img"
        with disk.open("xb") as f:
            f.truncate(8 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        for name in ("uname", "arch"):
            subprocess.run(
                ["mcopy", "-i", disk, work / name, "::" + name + ".elf"], check=True
            )
        subprocess.run(["mcopy", "-i", disk, probe, "::"], check=True)
        log = proof / (arch + ".serial")
        vm = Shell(
            qemu.boot_args(kernel, arch, work / "boot", 2, mode, esp_mib=36)
            + ["-drive", f"if=ide,format=raw,file={disk}"],
            log,
            work,
        )
        # GNU field order is independent of option order. Inspect the reference
        # host's dynamic fields separately; compare guest fields with guest uname().
        cases = [
            ("", "s"),
            ("-m", "m"),
            ("-n", "n"),
            ("-o", "o"),
            ("-s", "s"),
            ("-r", "r"),
            ("-v", "v"),
            ("-mrsn", "snrm"),
            ("-a", "snrvmo"),
        ]
        try:
            vm.wait(b"A:\\> ")
            vm.command("c:")
            vm.command("uts.elf > uts.bin")
            vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
            for i, (flags, _) in enumerate(cases):
                vm.command(f"uname.elf {flags} > u{i}.txt")
                vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
            vm.command("arch.elf > a.txt")
            vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
        finally:
            vm.close()

        def data(path):
            return subprocess.check_output(["mcopy", "-i", disk, "::" + path, "-"])

        wire = data("uts.bin")
        if len(wire) != 6 * 65:
            raise RuntimeError("malformed uname wire record")
        s, n, r, v, m, _ = [
            wire[i : i + 65].split(b"\0", 1)[0] for i in range(0, len(wire), 65)
        ]
        if (s, n, m) != (b"DOS-RS", b"dos-rs", arch.encode()) or not r or not v:
            raise RuntimeError("system identity does not match boot target")
        guest_fields = {"s": s, "n": b"guest-node", "r": r, "v": v, "m": m, "o": s}
        if b"GNU coreutils) 9.12" not in subprocess.check_output(
            [gnu / "uname", "--version"]
        ):
            raise RuntimeError("GNU uname 9.12 required")
        oracle = {
            key: subprocess.check_output([gnu / "uname", "-" + key]).rstrip(b"\n")
            for key in "snrvmo"
        }
        for i, (flag, keys) in enumerate(cases):
            guest = data(f"u{i}.txt")
            if guest != b" ".join(guest_fields[key] for key in keys) + b"\n":
                raise RuntimeError(f"{arch} {flag}: {guest!r}")
            if flag != "-a":
                host = subprocess.check_output(
                    [gnu / "uname"] + ([flag] if flag else [])
                )
                if host != b" ".join(oracle[key] for key in keys) + b"\n":
                    raise RuntimeError(f"GNU field ordering failed for {flag}")
        if data("a.txt") != m + b"\n":
            raise RuntimeError("arch differs from uname -m")
        result = {
            "arch": arch,
            "firmware": mode,
            "kernel_sha256": hashlib.sha256(kernel.read_bytes()).hexdigest(),
            "guest": {k: v.decode() for k, v in guest_fields.items()},
            "boot_nodename": n.decode(),
            "gnu_field_sha256": {k: hashlib.sha256(v).hexdigest() for k, v in oracle.items()},
            "cases": cases,
        }
        (proof / (arch + ".json")).write_text(json.dumps(result, indent=2) + "\n")
        print(arch, mode, "uname/arch dynamic fields and nodename passed", flush=True)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--ram-dir", required=True, type=Path)
    p.add_argument("--proof", type=Path, default=qemu.ROOT / "target/native-uts-proof")
    p.add_argument("--gnu-bin", required=True, type=Path)
    p.add_argument("--archive", required=True, type=Path)
    p.add_argument("--arch", action="append", choices=qemu.ARCH)
    a = p.parse_args()
    a.proof.mkdir(parents=True, exist_ok=True)
    for arch in a.arch or qemu.ARCH:
        verify(arch, a.ram_dir, a.proof, a.gnu_bin, a.archive)
