"""Boot the native shell with a read-only ISO 9660 CD and a writable FAT16 RAM disk."""

import argparse
import hashlib
import importlib.util
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

spec = importlib.util.spec_from_file_location(
    "files", Path(__file__).with_name("realmode-command.py")
)
files = importlib.util.module_from_spec(spec)
spec.loader.exec_module(files)


def run(image, ram, proof):
    proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ram) as folder:
        work = Path(folder)
        boot = work / "boot.img"
        shutil.copyfile(image, boot)
        disk = work / "scratch.img"
        volume = files.image(disk)
        root = work / "source"
        docs = root / "DOCS"
        sub = docs / "NEST"
        sibling = root / "OTHER"
        sub.mkdir(parents=True)
        sibling.mkdir()
        payload = bytearray(bytes(range(256)) * 301 + b"end")
        payload[65538:65542] = b"ABCD"  # FILES.COM seeks here and writes the four bytes to C:\API.TXT.
        (docs / "ONE.TXT").write_bytes(b"native optical first\r\n")
        (docs / "TWO.TXT").write_bytes(b"native optical second\r\n")
        (sub / "INFO.TXT").write_bytes(b"inside ISO subdirectory\r\n")
        (sub / "DATA.BIN").write_bytes(payload)
        shutil.copyfile(Path(__file__).resolve().parents[1] / "tests/fixtures/dos/FILES.COM", sub / "FILES.COM")
        os.utime(sub / "DATA.BIN", (1049551200, 1049551200))
        (sibling / "THIRD.TXT").write_bytes(b"separate directory\r\n")
        for i in range(66):
            (sub / f"F{i:03}.DAT").write_bytes(bytes([i]))
        iso = work / "native.iso"
        subprocess.run(
            [
                "bsdtar",
                "--format",
                "iso9660",
                "--options",
                "iso9660:volume-id=DOSOPTICAL",
                "-cf",
                iso,
                "-C",
                root,
                ".",
            ],
            check=True,
            env=dict(os.environ, TZ="UTC"),
        )
        original = hashlib.sha256(iso.read_bytes()).digest()
        commands = [
            "@ECHO OFF",
            "DIR D:\\DOCS > C:\\DIR.OUT",
            "DIR /B D:\\DOCS\\*.TXT > C:\\TEXT.OUT",
            "DIR /S /B D:\\*.TXT > C:\\TREE.OUT",
            "DIR /B D:\\DOCS\\NEST\\F???.DAT > C:\\MULTI.OUT",
            "D:",
            "CD DOCS",
            "TYPE NEST\\INFO.TXT > C:\\TYPE.OUT",
            "COPY /B NEST\\DATA.BIN C:\\BIG.BIN",
            r"DIR NEST\DATA.BIN > C:\SRCSTAMP.OUT",
            r"DIR C:\BIG.BIN > C:\DSTSTAMP.OUT",
            r"D:\DOCS\NEST\FILES.COM >C:\EXEC.OUT",
            r"FOR %%F IN (D:\DOCS\*.TXT) DO DIR /B D:\OTHER\*.TXT >>C:\PAIRED.OUT",
            "C:",
            "TYPE D:ONE.TXT > CURRENT.OUT",
            "DIR /B D:\\OTHER > OTHER.OUT",
            "TYPE D:TWO.TXT > AFTER.OUT",
            "COPY NEW.TXT D:\\DOCS\\FAIL.TXT",
            "IF EXIST D:\\DOCS\\FAIL.TXT ECHO WRITE-BREACH",
            "ECHO OPTICAL-END",
        ]
        files.mtool("mcopy", volume, "-", "::NEW.TXT", data=b"must remain on C:")
        files.mtool(
            "mcopy",
            boot,
            "-o",
            "-",
            "::AUTOEXEC.BAT",
            data=("\r\n".join(commands) + "\r\n").encode(),
        )
        serial = proof / "serial.txt"
        serial.unlink(missing_ok=True)
        with (
            (proof / "qemu.log").open("wb") as trace,
            subprocess.Popen(
                [
                    "qemu-system-i386",
                    "-machine",
                    "pc,accel=tcg",
                    "-cpu",
                    "486",
                    "-m",
                    "16",
                    "-nic",
                    "none",
                    "-display",
                    "none",
                    "-monitor",
                    "stdio",
                    "-serial",
                    "file:" + str(serial),
                    "-boot",
                    "a",
                    "-drive",
                    f"if=floppy,format=raw,readonly=on,file={boot}",
                    "-drive",
                    f"if=ide,index=0,format=raw,file={disk}",
                    "-drive",
                    f"if=ide,index=2,media=cdrom,readonly=on,format=raw,file={iso}",
                ],
                stdin=subprocess.PIPE,
                stdout=trace,
                stderr=trace,
            ) as vm,
        ):
            try:
                end = time.monotonic() + 100
                while vm.poll() is None and time.monotonic() < end:
                    if serial.exists() and b"OPTICAL-END\r\n" in serial.read_bytes():
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError(f"optical batch did not finish; see {serial}")
            finally:
                if vm.poll() is None:
                    vm.stdin.write(b"quit\n")
                    vm.stdin.flush()
                    vm.wait(10)
        for name in (
            "DIR",
            "TEXT",
            "TREE",
            "MULTI",
            "TYPE",
            "CURRENT",
            "OTHER",
            "AFTER",
            "EXEC",
            "PAIRED",
        ):
            data = files.mtool("mcopy", volume, "::" + name + ".OUT", "-").stdout
            (proof / (name + ".OUT")).write_bytes(data)
        assert (proof / "TEXT.OUT").read_bytes().splitlines() == [
            b"ONE.TXT",
            b"TWO.TXT",
        ]
        assert (proof / "MULTI.OUT").read_bytes().splitlines() == [
            f"F{i:03}.DAT".encode() for i in range(66)
        ]
        expected = {
            b"D:\\DOCS\\ONE.TXT",
            b"D:\\DOCS\\TWO.TXT",
            b"D:\\DOCS\\NEST\\INFO.TXT",
            b"D:\\OTHER\\THIRD.TXT",
        }
        assert set((proof / "TREE.OUT").read_bytes().splitlines()) == expected
        summary = (proof / "DIR.OUT").read_bytes()
        assert (
            b"DOSOPTICAL" in summary
            and b"<DIR>" in summary
            and b"0 bytes free" in summary
        )
        assert (proof / "TYPE.OUT").read_bytes() == (sub / "INFO.TXT").read_bytes()
        assert (proof / "CURRENT.OUT").read_bytes() == (docs / "ONE.TXT").read_bytes()
        assert (proof / "AFTER.OUT").read_bytes() == (docs / "TWO.TXT").read_bytes()
        assert (proof / "OTHER.OUT").read_bytes().splitlines() == [b"THIRD.TXT"]
        assert (proof / "EXEC.OUT").read_bytes() == b"INT21 7.10 FILE API OK\r\n"
        assert files.mtool("mcopy", volume, "::API.TXT", "-").stdout == b"ABCD"
        copied = files.mtool("mcopy", volume, "::BIG.BIN", "-").stdout
        assert copied == payload

        def stamp(name):
            content = files.mtool("mcopy", volume, "::" + name + ".OUT", "-").stdout
            (proof / (name + ".OUT")).write_bytes(content)
            found = re.search(rb"77059 +([0-9-]+) +([0-9:]+[ap])", content)
            assert found and found.groups() == (b"04-05-03", b"2:00p"), content

        stamp("SRCSTAMP")
        stamp("DSTSTAMP")
        assert (
            (proof / "PAIRED.OUT").read_bytes().splitlines()
            == [b"THIRD.TXT"] * 2
        )
        assert hashlib.sha256(iso.read_bytes()).digest() == original
        assert (
            b"Write protect error" in serial.read_bytes()
            and b"WRITE-BREACH" not in serial.read_bytes()
        )
        print(
            f"PASS ISO 9660 read-only ATAPI, nested DIR and independent DTAs, 66 records, TYPE, EXEC and COPY {len(copied)} bytes SHA-256 {hashlib.sha256(copied).hexdigest()}"
        )


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    for param in ("image", "ram-dir", "proof"):
        cli.add_argument("--" + param, required=True, type=Path)
    options = cli.parse_args()
    run(options.image, options.ram_dir, options.proof.resolve())
