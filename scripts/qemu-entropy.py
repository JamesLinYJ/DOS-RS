#!/usr/bin/env python3
"""Verify the native entropy source, missing source and CPU fallback on real guest ELFs."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


def run(arch, ram, proof):
    built = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    mode = "bios" if arch == "i686" else "uefi"
    with tempfile.TemporaryDirectory(dir=ram) as tmp:
        work = Path(tmp)
        kernel = work / "kernel.efi"
        kernel.write_bytes(built.read_bytes())
        binary = work / "rngtest.elf"
        subprocess.run([sys.executable, qemu.ROOT / "ports/musl/cc.py", "--arch", arch, "-o", binary,
                        qemu.ROOT / "tests/native-entropy/random.c"], check=True)
        for source, flags in [
            ("absent", ["-cpu", "qemu32,-rdrand,-rdseed" if arch == "i686" else "qemu64,-rdrand,-rdseed"]),
            ("virtio", ["-cpu", "qemu32,-rdrand,-rdseed" if arch == "i686" else "qemu64,-rdrand,-rdseed", "-device", "virtio-rng-pci"]),
            ("hardware", ["-cpu", "max"]),
        ]:
            case = work / source
            case.mkdir()
            disk = case / "c.img"
            with disk.open("xb") as f: f.truncate(8 << 20)
            subprocess.run(["mformat", "-i", disk, "::"], check=True)
            subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
            vm = Shell(qemu.boot_args(kernel, arch, case/"boot", 2, mode, esp_mib=36) + flags +
                       ["-drive", f"if=ide,format=raw,file={disk}"], proof/(arch+"-"+source+".serial"), case)
            try:
                vm.wait(b"A:\\> "); vm.command("c:")
                vm.command("rngtest.elf " + source + " > rng.bin")
                vm.command("echo $?", expect=b"echo $?\r\n0\r\n")
            finally:
                vm.close()
            data = subprocess.check_output(["mcopy", "-i", disk, "::rng.bin", "-"])
            if source == "absent":
                assert data == b"ENODEV\n"
            elif len(data) != 512 or data[:256] == data[256:]:
                raise RuntimeError("stopped disk entropy comparison failed")
            if source == "virtio" and b"Entropy source: VirtIO RNG" not in vm.log.read_bytes():
                raise RuntimeError("virtio device was not negotiated")
            (proof/(arch+"-"+source+".json")).write_text(json.dumps({
                "arch":arch, "firmware":mode, "source":source, "flags":flags,
                "kernel":hashlib.sha256(kernel.read_bytes()).hexdigest(), "bytes":len(data),
                "distinct_buffers": source != "absent", "output_digest":hashlib.sha256(data).hexdigest()
            }, indent=2)+"\n")
            print(arch, source, "passed", flush=True)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--ram-dir", required=True, type=Path)
    p.add_argument("--proof", type=Path, default=qemu.ROOT/"target/native-entropy-proof")
    p.add_argument("--arch", choices=qemu.ARCH, action="append")
    args = p.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or qemu.ARCH:
        run(arch,args.ram_dir,args.proof)
