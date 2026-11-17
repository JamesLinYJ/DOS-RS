"""Check consecutive conventional and LOADHIGH EXEC calls on native 286 and 386 DOS."""
import argparse
import os
from pathlib import Path
import re
import runpy
import struct
import subprocess
import tempfile

from realmode_probe import build_example

ROOT = Path(__file__).resolve().parents[1]
ownership = runpy.run_path(str(Path(__file__).with_name("realmode-file-ownership.py")))


def build_com(tools, work):
    output = work / "pspplace.com"
    subprocess.run([tools / "smlrcc", "-dost", "-SI", "v0100/include", "-I", "v0100/srclib",
                    "-SL", "v0100/lib", "-entry", "_probe", "-o", output,
                    ROOT / "tests/fixtures/dos/pspplace.c"], check=True, cwd=tools,
                   env=dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                            TMPDIR=str(work)))
    return output


def check(args, cpu, probe):
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        work = Path(tmp).resolve()
        disk = work / "drive.img"
        cyl = 16
        with disk.open("wb") as f:
            f.truncate(cyl * 16 * 63 * 512)
        subprocess.run([ownership["fat_tool"]("mkfs.fat"), "-F", "16", "-s", "1", disk], check=True)
        if cpu == "286":
            binary, source = "PLACE.EXE", probe
            pattern = rb"PSP=([0-9A-F]{4}) SIZE="
        else:
            # Smaller C's tiny-model code requires a 386 even when running in real mode.
            binary, source = "PSPPLACE.COM", build_com(args.smaller_c.resolve(), work)
            pattern = rb"PSP=([0-9A-F]{4}) OWNER/ENV/IO OK\r\n"
        ownership["mcopy"](disk, source, "::" + binary)
        batch = [binary, binary, "LH " + binary, "LOADHIGH " + binary, binary]
        log = ownership["boot"](work, args.proof.resolve(), cpu, cpu, cyl, batch,
                                args.image.read_bytes(), args.dosbox)
        psp = [int(seg, 16) for seg in re.findall(pattern, log)]
        if len(psp) != 5 or psp[0] != psp[1] or psp[0] != psp[4] or psp[2] != psp[3]:
            raise AssertionError(f"{cpu}: process placement or cleanup differs: {psp}")
        if psp[0] >= 0xa000 or (psp[2] >= 0xa000) != (cpu == "386"):
            raise AssertionError(f"{cpu}: incorrect conventional/upper placement: {psp}")
        if cpu == "286":
            data = probe.read_bytes()
            last, pages, _, header, _, maximum = struct.unpack_from("<6H", data, 2)
            size = ((pages - 1) * 512 + (last or 512) + 15) // 16 - header
            granted = [int(n, 16) for n in re.findall(rb"SIZE=([0-9A-F]{4})", log)]
            if granted != [16 + size + maximum] * 5:
                raise AssertionError(f"EXEC did not honor the MZ allocation limit: {granted}")
        if cpu == "386" and ownership["mcopy"](disk, "::PSPPLACE.TMP", "-") != b"UMB file":
            raise AssertionError("stopped disk does not contain the COM client's payload")
        print(f"Native {cpu}: " + ", ".join(f"{seg:04X}" for seg in psp), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("toolchain", "smaller-c", "image", "ram-dir", "proof"):
        parser.add_argument("--" + arg, required=True, type=Path)
    parser.add_argument("--dosbox", default="dosbox-x")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    probe = build_example(args.toolchain, "umb_placement")
    for cpu in ("286", "386"):
        check(args, cpu, probe)
