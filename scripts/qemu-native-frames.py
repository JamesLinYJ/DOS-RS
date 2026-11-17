"""Exhaust firmware-donated RAM from ring 3, fault on retired pages, then check reclamation."""
import argparse
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell


def run(arch, cpus, kernel, ram, proof, memory):
    firmware = "bios" if arch == "i686" else "uefi"
    label = f"{arch}-{firmware}-smp{cpus}-{memory}MiB"
    with tempfile.TemporaryDirectory(dir=ram) as tmp:
        work = Path(tmp)
        disk = work / "c.img"
        with disk.open("xb") as stream:
            stream.truncate(4 << 20)
        subprocess.run(["mformat", "-i", disk, "::"], check=True)
        for name in ("frames", "hello"):
            binary = work / (name + ".elf")
            subprocess.run([sys.executable, qemu.ROOT / "scripts/native-cc.py", "--arch", arch,
                            "-o", binary, qemu.ROOT / "native-exec/examples" / (name + ".c")], check=True)
            subprocess.run(["mcopy", "-i", disk, binary, "::"], check=True)
        args = qemu.boot_args(kernel, arch, work / "boot", cpus, firmware, esp_mib=36)
        args[args.index("-m") + 1] = str(memory)
        vm = Shell(args + ["-accel", "tcg,thread=multi", "-drive", f"if=ide,format=raw,file={disk}"],
                   proof / (label + ".serial"), work)
        try:
            vm.wait(b"A:\\> ")
            vm.command("c:")
            vm.command("frames.elf first", expect=b"all physical frames used, ENOMEM and zeroed reuse OK")
            vm.command("frames.elf stale", expect=b"user exception 0e")
            vm.command("echo $?", expect=b"\r\n142\r\n")
            vm.command("frames.elf again", expect=b"all physical frames used, ENOMEM and zeroed reuse OK")
            vm.command("hello.elf 7", expect=b"hello from native C")
        finally:
            vm.close()
        records = [subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])
                   for name in ("first.bin", "again.bin")]
        if records[0] != records[1]:
            raise RuntimeError(f"{label}: page counts changed after exit and fault")
        total, free, initial, peak, remaining = struct.unpack("<5I", records[0])
        if not (total >= free > 512 and initial + free == peak and remaining == 0):
            raise RuntimeError(f"{label}: invalid physical or process page counts")
        (proof / (label + ".bin")).write_bytes(records[0])
        print(f"{label}: {total} donated pages, {free} allocated, zero free at ENOMEM; "
              "same count after exit, page fault and relaunch", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", required=True, type=Path)
    parser.add_argument("--arch", choices=("i686", "x86_64"), action="append")
    parser.add_argument("--smp", choices=(1, 4), type=int, action="append")
    parser.add_argument("--memory", type=int, help="guest RAM in MiB (160 BIOS, 256 UEFI by default)")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    for arch in args.arch or ("i686", "x86_64"):
        kernel = qemu.build(arch, qemu.ROOT / "target/native-frames")
        for cpus in args.smp or (1, 4):
            memory = args.memory if args.memory is not None else (160 if arch == "i686" else 256)
            run(arch, cpus, kernel, args.ram_dir, args.proof, memory)
