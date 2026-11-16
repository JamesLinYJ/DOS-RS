"""Build one native BIOS FAT12 disk with an 8086 loader and both DOS-RS kernels."""

import argparse
import os
import struct
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def relocated(path, segment):
    data = path.read_bytes()

    def word(offset):
        return struct.unpack_from("<H", data, offset)[0]

    if data[:2] != b"MZ" or word(20) or word(22):
        raise ValueError(f"{path}: expected a base-relative entry at 0000:0000")
    header = 16 * word(8)
    size = word(4) * 512 - ((512 - word(2)) % 512)
    if size != len(data) or header < word(24) + 4 * word(6):
        raise ValueError("invalid MZ header")
    image = bytearray(data[header:])
    for n in range(word(6)):
        offset, paragraph = struct.unpack_from("<HH", data, word(24) + 4 * n)
        at = 16 * paragraph + offset
        (value,) = struct.unpack_from("<H", image, at)
        struct.pack_into("<H", image, at, value + segment)
    return image


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--toolchain", required=True, type=Path)
    cli.add_argument("--output", required=True, type=Path)
    cli.add_argument(
        "--file",
        action="append",
        type=Path,
        default=[],
        help="additional file to copy into the root directory",
    )
    args = cli.parse_args()
    tools = args.toolchain.resolve()
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    target = ROOT / "target/native16"
    target.mkdir(parents=True, exist_ok=True)

    def run(*cmd, **kwargs):
        subprocess.run(cmd, check=True, **kwargs)

    bios = tools / "target/release/bios16"

    def compile(cpu, manifest, name, paragraph):
        build = target / cpu
        run(
            tools / "scripts/cargo16.sh",
            "--release",
            "--locked",
            "--manifest-path",
            manifest,
            "--target-dir",
            build,
            cwd=tools,
            env=dict(
                os.environ,
                RUST16_CPU=cpu,
                RUST16_PLATFORM="bios-loader" if cpu == "8086" else "bios-kernel",
            ),
        )
        return relocated(build / "dos16/release" / name, paragraph)

    loader = compile(
        "8086", ROOT / "boot/realmode/Cargo.toml", "dosrs-realmode.exe", 0x0800
    )
    count = (len(loader) + 511) // 512
    kernels = {
        f"K{cpu}.SYS": compile(cpu, ROOT / "Cargo.toml", "dos.exe", 0x1000)
        for cpu in ("286", "386")
    }
    with tempfile.TemporaryDirectory(dir=output.parent) as tmp:
        d = Path(tmp)
        boot = d / "sector"
        run(bios, boot, str(count))
        volume = bytearray(1440 * 1024)
        volume[:512] = boot.read_bytes()
        volume[3:11] = b"DOSRS16 "
        reserved = count + 1
        struct.pack_into(
            "<HBHBHHBHHHII",
            volume,
            11,
            512,
            1,
            reserved,
            2,
            224,
            2880,
            0xF0,
            9,
            18,
            2,
            0,
            0,
        )
        struct.pack_into(
            "<BBBI11s8s",
            volume,
            36,
            0,
            0,
            0x29,
            0x16386286,
            b"DOSRS16    ",
            b"FAT12   ",
        )
        volume[512 : 512 + len(loader)] = loader
        for lba in (reserved, reserved + 9):
            volume[lba * 512 : lba * 512 + 3] = b"\xf0\xff\xff"
        disk = d / "native.img"
        disk.write_bytes(volume)
        for name, data in kernels.items():
            path = d / name
            path.write_bytes(data)
            run("mcopy", "-i", disk, path, "::" + name)
        for path in args.file:
            run("mcopy", "-i", disk, path.resolve(), "::" + path.name)
        disk.replace(output)
    print(output)


if __name__ == "__main__":
    main()
