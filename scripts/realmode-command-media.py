#!/usr/bin/env python3
"""Install the two-floppy COMMAND edition while its data disk exceeds all conventional memory."""

import argparse
import hashlib
from pathlib import Path
import runpy
import shutil
import struct
import subprocess
import tempfile
import time


def mcopy(image, source, dest, data=None):
    return subprocess.run(
        ["mcopy", "-o", "-i", image, source, dest],
        input=data,
        stdout=subprocess.PIPE,
        check=True,
    ).stdout


def prepare(first, second):
    # A larger welcome file exercises the normal low-memory path without changing
    # the executable or lying about the guest's PSP allocation / BIOS memory map.
    contents = bytes(range(256)) * 768
    mcopy(second, "-", "::WELCOME.TXT", contents)
    if mcopy(second, "::WELCOME.TXT", "-") != contents:
        raise ValueError("fixture does not fit on disk 2")
    manifest = bytearray(mcopy(first, "::CHECKSUM.DAT", "-"))
    row = next(
        offset
        for offset in range(4, len(manifest), 19)
        if manifest[offset : offset + 11] == b"WELCOME TXT"
    )
    a = b = 0
    for (word,) in struct.iter_unpack("<H", contents):
        a = (a + word) % 65535
        b = (b + a) % 65535
    struct.pack_into("<IHH", manifest, row + 11, len(contents), a, b)
    mcopy(first, "-", "::CHECKSUM.DAT", manifest)
    release = hashlib.sha256(manifest).digest()[:16]
    for n, disk in enumerate((first, second), 1):
        mcopy(disk, "-", f"::DISK{n}.ID", release)
    return contents


def wait_any(session, names):
    until = time.monotonic() + 100
    while time.monotonic() < until and session.proc.poll() is None:
        data = session.log.read_bytes()
        for label in names:
            key = label.encode()
            at = data.find(key, session.mark)
            if at >= 0:
                session.mark = at + len(key)
                return label
        time.sleep(0.08)
    raise RuntimeError(
        f"{session.log}: missing {names}; tail={session.log.read_bytes()[-200:]!r}"
    )


def swapped_install(vm, first, second, hard, proof):
    counts = {1: 0, 2: 0}
    with vm(first, [hard], proof, "streaming-install") as s:
        s.type("setup\n")
        s.expect("1-8 >")
        s.key("2")
        s.expect("Disk >")
        s.key("1")
        s.expect("Action >")
        s.key("1")
        controls = [
            "Insert disk 2 >",
            "Restore disk 1 >",
            "Config >",
            "Confirm >",
            "Installation complete",
        ]
        while True:
            prompt = wait_any(s, controls)
            if prompt == "Installation complete":
                break
            if prompt in controls[:2]:
                n = 2 if prompt == controls[0] else 1
                disk = second if n == 2 else first
                quoted = str(disk).replace("\\", "\\\\").replace('"', '\\"')
                s.monitor(f'change setupfd "{quoted}" raw read-only')
                counts[n] += 1
            s.key("ret")
    if counts[1] < 3 or counts[2] < 3:
        raise AssertionError(
            f"expected real streaming swaps in preflight and copy: {counts}"
        )
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["disk-set", "ram-dir", "proof"]:
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    proof = args.proof.resolve()
    proof.mkdir(parents=True, exist_ok=True)
    install = runpy.run_path(str(Path(__file__).with_name("realmode-install-test.py")))
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as temp:
        work = Path(temp)
        first = work / "disk1.img"
        second = work / "disk2.img"
        hard = install["disk_file"](work, "hd.img", 16)
        for origin, dest in [
            (args.disk_set / "DISK1.IMG", first),
            (args.disk_set / "DISK2.IMG", second),
        ]:
            if origin.stat().st_size != 1440 * 1024:
                raise ValueError("expected 1.44 MiB media")
            shutil.copyfile(origin, dest)
        expected = prepare(first, second)
        counts = swapped_install(install["vm"], first, second, hard, proof)
        volume = str(hard) + "@@1048576"
        if mcopy(volume, "::WELCOME.TXT", "-") != expected:
            raise AssertionError("streamed data differs on installed disk")
        for name in ["EDIT.EXE", "INPUT.DAT", "SMARTDRV.EXE"]:
            if mcopy(second, "::" + name, "-") != mcopy(volume, "::" + name, "-"):
                raise AssertionError(name)
        with install["vm"](None, [hard], proof, "streaming-cold-boot") as s:
            s.type("ver\n")
            s.expect("C:\\> ")
        second_hd = install["disk_file"](work, "dual-drive.img", 16)
        with install["vm"](
            first, [second_hd], proof, "two-drive-install", second=second
        ) as s:
            s.wizard("setup", 1, 1)
            s.key("ret")
            s.expect("Installation complete")
            if b"Insert disk 2 >" in s.log.read_bytes():
                raise AssertionError("mounted drive B was not used")
        if mcopy(str(second_hd) + "@@1048576", "::WELCOME.TXT", "-") != expected:
            raise AssertionError("drive B payload differs")
        print("PASS two drives, disk 2 larger than process memory, installed file hash")
        print(
            "PASS streaming preflight, single-drive swaps, installed file hash, cold boot",
            counts,
        )


if __name__ == "__main__":
    main()
