"""Build one native BIOS FAT12 disk with an 8086 loader and both DOS-RS kernels."""

import argparse
from lz4block import compress
import hashlib
import os
import json
import shutil
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
    cli.add_argument("--output", type=Path, help="single bootable installation floppy")
    cli.add_argument(
        "--disk-set", type=Path, help="directory for DISK1.IMG and DISK2.IMG"
    )
    cli.add_argument(
        "--file",
        action="append",
        type=Path,
        default=[],
        help="additional file to copy into the root directory",
    )
    cli.add_argument(
        "--system-dir",
        type=Path,
        help="also export the DOS system and installation files",
    )
    args = cli.parse_args()
    tools = args.toolchain.resolve()
    if not args.output and not args.disk_set:
        cli.error("choose --output or --disk-set, or both")
    output = args.output.resolve() if args.output else None
    disk_set = args.disk_set.resolve() if args.disk_set else None
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
    if disk_set:
        disk_set.mkdir(parents=True, exist_ok=True)
    target = ROOT / "target/native16"
    target.mkdir(parents=True, exist_ok=True)

    def run(*cmd, **kwargs):
        subprocess.run(cmd, check=True, **kwargs)

    run("python3", ROOT / "scripts/realmode-emm.py", "--toolchain", tools)
    ems = target / "emm"
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
                RUST16_OBJECTS=str(ems / "device.obj") if cpu == "8086" else str(ems / "upper.obj"),
                RUST16_PLATFORM="bios-loader" if cpu == "8086" else "bios-kernel",
            ),
        )
        return relocated(build / "dos16/release" / name, paragraph)

    loader = compile(
        "8086", ROOT / "boot/realmode/Cargo.toml", "dosrs-realmode.exe", 0x0100
    )
    if len(loader) > 0xF000:
        raise ValueError("IO.SYS would overlap the kernel")
    kernels = [
        compile(cpu, ROOT / "Cargo.toml", "dos.exe", 0x1000) for cpu in ("286", "386")
    ]
    first = 512
    packed286 = compress(kernels[0])
    second = first + len(packed286)
    header = struct.pack(
        "<4sIHIH", b"DOSK", first | 0x80000000, len(kernels[0]),
        second | 0x80000000, len(kernels[1])
    )
    system = header.ljust(first, b"\0") + packed286 + compress(kernels[1])
    util = target / "util"
    util.mkdir(exist_ok=True)
    obj = util / "io.obj"
    run(bios, "--dos-io", obj)
    run(
        tools / "scripts/cargo16.sh",
        "--locked",
        "--release",
        "--manifest-path",
        ROOT / "native-tools/Cargo.toml",
        "--target-dir",
        util,
        env=dict(
            os.environ, RUST16_OBJECTS=os.pathsep.join(map(str, [obj, ems / "client.obj"])), RUST16_CPU="286", RUST16_PLATFORM="dos"
        ),
    )
    with tempfile.TemporaryDirectory(dir=output.parent if output else disk_set) as tmp:
        d = Path(tmp)
        (d / "IO.SYS").write_bytes(loader)
        (d / "MSDOS.SYS").write_bytes(system)
        for name, extension in [
            ("command", "COM"),
            ("setup", "EXE"),
            ("fdisk", "EXE"),
            ("smartdrv", "EXE"),
            ("memstat", "EXE"),
            ("edit", "EXE"),
        ]:
            shutil.copyfile(
                util / "dos16/release" / (name + ".exe"),
                d / (name.upper() + "." + extension),
            )
        if (d / "COMMAND.COM").stat().st_size > 0x7000:
            raise ValueError("COMMAND.COM exceeds its 28 KiB resident image cache")
        run(bios, "--mbr", d / "mbr")
        run(bios, "--vbr-at", "100", d / "vbr")
        (d / "BOOT.BIN").write_bytes(
            (d / "mbr").read_bytes() + (d / "vbr").read_bytes()
        )
        for name in ["GLYPHS.FNT", "INPUT.DAT"]:
            shutil.copyfile(ROOT / "native-tools/assets" / name, d / name)
        (d / "OFL.TXT").write_bytes(
            (ROOT / "native-tools/assets/OFL.TXT")
            .read_text()
            .replace("\n", "\r\n")
            .encode()
        )
        langs = json.loads((ROOT / "native-tools/locales.json").read_text())
        if len(langs) != 8 or any(len(lang) != 103 for lang in langs):
            raise ValueError("installer message catalog has an unexpected size")
        strings = [msg.encode("gb18030") for lang in langs for msg in lang]
        pos = 6 * len(strings)
        table = bytearray()
        for value in strings:
            table += struct.pack("<IH", pos, len(value))
            pos += len(value)
        (d / "LANG.DAT").write_bytes(table + b"".join(strings))
        (d / "WELCOME.TXT").write_bytes(
            "DOS   中文 繁體 日本語 한국어 Français\r\nGB18030   中文 㐀 𠀀\r\n".encode(
                "gb18030"
            )
        )
        (d / "LICENSE.TXT").write_bytes(
            (ROOT / "LICENSE").read_text().replace("\n", "\r\n").encode()
        )
        (d / "NOTICE.TXT").write_bytes(
            (ROOT / "NOTICE").read_text().replace("\n", "\r\n").encode()
        )
        shutil.copyfile(ems / "EMM386.SYS", d / "EMM386.SYS")
        (d / "CONFIG.SYS").write_bytes(b"DEVICE=EMM386.SYS\r\nDOS=UMB\r\n")
        files = [
            d / n
            for n in [
                "IO.SYS",
                "MSDOS.SYS",
                "COMMAND.COM",
                "SETUP.EXE",
                "FDISK.EXE",
                "SMARTDRV.EXE",
                "BOOT.BIN",
                "LANG.DAT",
                "GLYPHS.FNT",
                "OFL.TXT",
                "WELCOME.TXT",
                "INPUT.DAT",
                "LICENSE.TXT",
                "NOTICE.TXT",
                "EDIT.EXE",
                "EMM386.SYS",
                "MEMSTAT.EXE",
            ]
        ]
        # File identities, lengths and Fletcher-32 over little-endian words.
        manifest = bytearray(b"DSV1")
        for f in files:
            payload = f.read_bytes()
            short, ext = f.name.split(".")
            a = b = 0
            for (word,) in struct.iter_unpack(
                "<H", payload + b"\0" * (len(payload) % 2)
            ):
                a = (a + word) % 65535
                b = (b + a) % 65535
            manifest.extend(
                short.ljust(8).encode()
                + ext.ljust(3).encode()
                + struct.pack("<IHH", len(payload), a, b)
            )
        (d / "CHECKSUM.DAT").write_bytes(manifest)
        files.append(d / "CHECKSUM.DAT")
        if not any(f.name.upper() == "CONFIG.SYS" for f in args.file):
            files.append(d / "CONFIG.SYS")

        def floppy(disk, payload, volume, bootable):
            with disk.open("xb") as stream:
                stream.truncate(1474560)
            run("mformat", "-i", disk, "-f", "1440", "-v", volume, "::")
            for f in payload:
                run("mcopy", "-i", disk, f, "::" + f.name)
            if not bootable:
                return
            for name in ("IO.SYS", "MSDOS.SYS"):
                run("mattrib", "-i", disk, "+s", "+h", "+r", "::" + name)
            with disk.open("r+b") as f:
                bpb = f.read(512)
                vbr = bytearray((d / "vbr").read_bytes())
                vbr[3:62] = bpb[3:62]
                reserved = struct.unpack_from("<H", bpb, 14)[0]
                fat = struct.unpack_from("<H", bpb, 22)[0]
                entries = struct.unpack_from("<H", bpb, 17)[0]
                root = reserved + bpb[16] * fat
                data = root + (entries + 15) // 16
                f.seek(root * 512)
                directory = f.read(entries * 32)
                entry = next(
                    directory[i : i + 32]
                    for i in range(0, len(directory), 32)
                    if directory[i : i + 11] == b"IO      SYS"
                )
                assert (
                    entry[:11] == b"IO      SYS"
                    and struct.unpack_from("<H", entry, 26)[0] == 2
                )
                struct.pack_into("<IH", vbr, 416, data, (len(loader) + 511) // 512)
                f.seek(0)
                f.write(vbr)

        ready = []
        if output:
            single = d / "system.img"
            floppy(single, files + args.file, "DOS_SETUP", True)
            ready.append((single, output))
        if disk_set:
            apps = [d / "EDIT.EXE", d / "WELCOME.TXT"]
            if sum(f.stat().st_size for f in apps) > 65520:
                raise ValueError("disk 2 payload exceeds the setup transfer arena")
            identity = hashlib.sha256(manifest).digest()[:16]
            for number in (1, 2):
                mark = d / f"DISK{number}.ID"
                mark.write_bytes(identity)
                part = d / f"DISK{number}.IMG"
                payload = (
                    ([f for f in files if f not in apps] + args.file)
                    if number == 1
                    else apps
                )
                floppy(part, payload + [mark], f"DOS_SETUP{number}", number == 1)
                ready.append((part, disk_set / part.name))
        for image, destination in ready:
            image.replace(destination)
            print(destination)
        if args.system_dir:
            args.system_dir.mkdir(parents=True, exist_ok=True)
            for f in files:
                shutil.copyfile(f, args.system_dir / f.name)


if __name__ == "__main__":
    main()
