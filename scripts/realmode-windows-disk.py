"""Install the candidate DOS on a new RAM C disk with an existing Chinese Windows installation."""
import argparse
import hashlib
import os
from pathlib import Path
import runpy
import struct
import subprocess
import tempfile

installer = runpy.run_path(str(Path(__file__).with_name("realmode-install-test.py")))


def partition(disk):
    with disk.open("rb") as stream:
        mbr = stream.read(512)
    if len(mbr) != 512 or mbr[510:] != b"\x55\xaa":
        raise ValueError(f"{disk} has no boot sector")
    if (mbr[0] in (0xeb, 0xe9) and struct.unpack_from("<H", mbr, 11)[0] == 512
            and mbr[13] in (1, 2, 4, 8, 16, 32, 64, 128) and mbr[16] in (1, 2)):
        return 0  # A raw FAT volume as mounted by the i686 and x86_64 kernels.
    for index in range(446, 510, 16):
        if mbr[index] == 0x80 and mbr[index + 4] in (1, 4, 6, 11, 12, 14):
            return struct.unpack_from("<I", mbr, index + 8)[0] * 512
    raise ValueError(f"{disk} has no active FAT boot partition")


def install(seed, image, output, ram, proof):
    source_offset = partition(seed)
    with tempfile.TemporaryDirectory(dir=ram) as directory:
        work = Path(directory)
        candidate = work / "installed.img"
        with candidate.open("xb") as stream:
            stream.truncate(seed.stat().st_size)
        with installer["vm"](image, [candidate], proof, "windows-setup") as vm:
            vm.wizard("setup", 1, 1)
            vm.key("ret")
            vm.expect("Installation complete")
        # Copy between FAT volumes so DOS attributes and file times survive the transfer.
        conf = work / "mtools.conf"
        entries = []
        for letter, disk, offset in (("s", seed, source_offset), ("t", candidate, partition(candidate))):
            escaped = str(disk).replace("\\", "\\\\").replace('"', '\\"')
            entries.append(f'drive {letter}: file="{escaped}" offset={offset}')
        conf.write_text("\n".join(entries) + "\n")
        env = dict(os.environ, MTOOLSRC=str(conf))
        subprocess.run(["mcopy", "-spmo", "s:/WINDOWS", "t:/"], check=True, env=env)
        for path in ("WINDOWS/WIN.COM", "WINDOWS/SYSTEM/WIN386.EXE", "WINDOWS/PROGMAN.EXE",
                     "WINDOWS/SYSTEM.INI"):
            original, copy = (subprocess.check_output(["mcopy", drive + ":/" + path, "-"], env=env)
                              for drive in ("s", "t"))
            if original != copy:
                raise RuntimeError(f"Windows fixture differs at {path}")
            print(f"{path} {len(copy)} bytes SHA256 {hashlib.sha256(copy).hexdigest()}")
        for path in ("CONFIG.SYS", "AUTOEXEC.BAT"):
            data = subprocess.check_output(["mcopy", "t:/" + path, "-"], env=env)
            print(f"{path} {len(data)} bytes SHA256 {hashlib.sha256(data).hexdigest()}")
        # Both paths are on RAM. Publish only a complete image, without overwriting an existing one.
        os.link(candidate, output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("seed", "image", "ram-dir", "proof"):
        parser.add_argument("--" + key, type=Path, required=True)
    parser.add_argument("--name", default="windows-candidate.img", help="new file name within --ram-dir")
    args = parser.parse_args()
    if not args.name or Path(args.name).name != args.name or args.name in (".", ".."):
        parser.error("--name must be a file name")
    ram = args.ram_dir.resolve(strict=True)
    output = ram / args.name
    if output.exists():
        parser.error(f"{output} already exists")
    proof = args.proof.resolve()
    proof.mkdir(parents=True, exist_ok=True)
    install(args.seed.resolve(strict=True), args.image.resolve(strict=True), output, ram, proof)
    print(output)
