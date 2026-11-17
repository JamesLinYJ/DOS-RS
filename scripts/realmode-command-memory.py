"""Measure conventional memory in a DOS child before and after replacing the resident shell."""
import argparse
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

from qemu_shell import Shell
from realmode_probe import build_example


def measure(disk, probe, ram, proof, version):
    with tempfile.TemporaryDirectory(dir=ram) as name:
        work = Path(name)
        seeded = work / "source.img"
        shutil.copyfile(disk, seeded)
        with seeded.open("rb") as stream:
            mbr = stream.read(512)
        entries = [mbr[i:i+16] for i in range(446, 510, 16)]
        part = next(e for e in entries if e[0] == 128 and e[4] in (4, 6, 11, 12, 14))
        volume = f"{seeded}@@{512 * struct.unpack_from('<I', part, 8)[0]}"
        subprocess.run(["mcopy", "-o", "-i", volume, probe, "::MAXBLOCK.EXE"], check=True)
        for arch, cpu, binary in (("i686", "pentium3", "qemu-system-i386"),
                                  ("x86_64", "qemu64", "qemu-system-x86_64")):
            overlay = work / (arch + ".qcow2")
            subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-F", "raw",
                            "-b", seeded, overlay], check=True)
            log = proof / f"{version}-{arch}.serial"
            vm = Shell([binary, "-machine", "pc", "-cpu", cpu, "-accel", "tcg",
                        "-m", "16", "-boot", "c", "-vga", "std", "-no-reboot",
                        "-drive", f"if=ide,format=qcow2,file={overlay}"], log, work)
            try:
                vm.wait(b"C:\\> ")
                vm.command("maxblock", expect=b"bytes. Exit=17")
                vals = re.findall(rb"MAXBLOCK initial=(\d+) allocated-and-readback=(\d+)", log.read_bytes())
                if len(vals) != 1:
                    raise RuntimeError(f"{version} {arch} missing measurement: {log}")
                print(version, arch, *(int(x) for x in vals[0]), flush=True)
            finally:
                vm.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("baseline", "candidate", "toolchain", "ram-dir", "proof"):
        parser.add_argument("--" + arg, required=True, type=Path)
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    probe = build_example(args.toolchain, "maxblock")
    for label, disk in (("baseline", args.baseline), ("candidate", args.candidate)):
        measure(disk, probe, args.ram_dir, args.proof, label)
